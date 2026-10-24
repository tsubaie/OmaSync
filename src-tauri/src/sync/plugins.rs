//! Plugins as Omarchy manages them: every one it knows here, on and off, adding one from git,
//! updating and removing. The app is a window over `omarchy plugin`.

use super::*;

/// Every plugin Omarchy knows on this computer, from its catalog (names, descriptions) and its
/// list (on or off), except the app's own bar indicator, which Settings manages.
fn omarchy_plugins(ctx: &Ctx) -> Vec<Plugin> {
    let json = |args: &[&str]| -> Vec<serde_json::Value> {
        run(None, "omarchy", args)
            .ok()
            .and_then(|out| serde_json::from_str::<serde_json::Value>(&out).ok())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
    };
    let state: BTreeMap<String, (bool, bool)> = json(&["plugin", "list", "--json"])
        .iter()
        .filter_map(|p| {
            Some((
                p["id"].as_str()?.to_string(),
                (
                    p["enabled"].as_bool().unwrap_or(false),
                    p["canDisable"].as_bool().unwrap_or(true),
                ),
            ))
        })
        .collect();
    json(&["plugin", "catalog"])
        .iter()
        .filter_map(|p| {
            let id = p["id"].as_str()?;
            if !is_plugin_id(id) || id == BAR_PLUGIN {
                return None;
            }
            let text = |k: &str| p[k].as_str().unwrap_or_default().trim().to_string();
            let (enabled, can_disable) = state.get(id).copied().unwrap_or((false, true));
            Some(Plugin {
                id: id.to_string(),
                name: if text("name").is_empty() {
                    id.to_string()
                } else {
                    text("name")
                },
                description: text("description"),
                kinds: p["kinds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|k| k.as_str().map(String::from))
                    .collect(),
                first_party: p["firstParty"].as_bool().unwrap_or(false),
                here: true,
                enabled: Some(enabled),
                can_disable,
                source: ctx
                    .plugin_source(id)
                    .or_else(|| omarchy_source(p["manifestPath"].as_str()?)),
                update: None,
            })
        })
        .collect()
}

/// Omarchy's own plugins ship as files, laid out as in Omarchy's repository: the folder there.
fn omarchy_source(manifest: &str) -> Option<String> {
    let rel = Path::new(manifest)
        .parent()?
        .strip_prefix("/usr/share/omarchy")
        .ok()?;
    Some(format!(
        "https://github.com/omacom/omarchy/tree/HEAD/{}",
        rel.display()
    ))
}

/// Which plugins from git have something newer at their source: true when the source's HEAD
/// differs from the copy here. One short question per plugin, all at once.
pub(crate) fn plugin_updates(ctx: &Ctx) -> BTreeMap<String, bool> {
    let dirs: Vec<(String, PathBuf)> = ctx
        .local_plugins()
        .into_iter()
        .map(|id| {
            let dir = ctx.home.join(PLUGIN_DIR).join(&id);
            (id, dir)
        })
        .filter(|(_, dir)| dir.join(".git").exists())
        .collect();
    std::thread::scope(|scope| {
        let asked: Vec<_> = dirs
            .iter()
            .map(|(id, dir)| {
                scope.spawn(move || {
                    let here = run(Some(dir), "git", &["rev-parse", "HEAD"]).ok()?;
                    let there = run(Some(dir), "git", &["ls-remote", "origin", "HEAD"]).ok()?;
                    Some((id.clone(), there.split_whitespace().next()? != here))
                })
            })
            .collect();
        asked
            .into_iter()
            .filter_map(|a| a.join().ok().flatten())
            .collect()
    })
}

/// The day's answer from `plugin_updates`, kept in the state folder.
#[derive(Serialize, Deserialize)]
struct Checked {
    at: i64,
    updates: BTreeMap<String, bool>,
}

fn checked_file(ctx: &Ctx) -> PathBuf {
    ctx.state_dir().join("omarchy-sync/plugin-updates.json")
}

/// `plugin_updates`, asked at most once a day: the app looks far more often than plugin makers
/// publish. An answer that came back empty (offline) isn't kept, so the next look asks again;
/// updating or adding a plugin forgets the day's answer (see `forget_plugin_updates`).
pub(crate) fn plugin_updates_today(ctx: &Ctx) -> BTreeMap<String, bool> {
    let path = checked_file(ctx);
    if let Ok(text) = std::fs::read_to_string(&path)
        && let Ok(checked) = serde_json::from_str::<Checked>(&text)
        && now() < checked.at + 86_400
    {
        return checked.updates;
    }
    let updates = plugin_updates(ctx);
    if !updates.is_empty() {
        let checked = Checked {
            at: now(),
            updates: updates.clone(),
        };
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
        let _ = std::fs::write(&path, serde_json::to_string(&checked).unwrap_or_default());
    }
    updates
}

pub(crate) fn forget_plugin_updates(ctx: &Ctx) {
    let _ = std::fs::remove_file(checked_file(ctx));
}

/// What the Plugins page lists: everything Omarchy knows here, plus the plugins other computers
/// have that this one doesn't, by name.
pub(crate) fn all_plugins(ctx: &Ctx, data: &Data) -> Vec<Plugin> {
    let mut plugins = omarchy_plugins(ctx);
    let mut ids: BTreeSet<String> = ctx.local_plugins().into_iter().collect();
    ids.extend(data.plugins.iter().cloned());
    ids.extend(
        data.computers
            .values()
            .flat_map(|c| c.plugins.iter().cloned()),
    );
    for id in ids {
        if is_plugin_id(&id) && id != BAR_PLUGIN && !plugins.iter().any(|p| p.id == id) {
            plugins.push(ctx.plugin(&id));
        }
    }
    for p in &mut plugins {
        if p.source.is_none() {
            p.source = data.plugin_sources.get(&p.id).cloned();
        }
    }
    plugins.sort_by_key(|p| p.name.to_lowercase());
    plugins
}

/// Takes other computers' plugins onto this one: it now counts as having them, so the ignore
/// rules let them through, and they're written out straight away. One that came from git is
/// added from git instead, so this computer's copy can be updated too.
pub fn install_plugins(ctx: &Ctx, ids: &[String]) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    let mut data = ctx.load_data();
    let me = data.computers.entry(ctx.host.clone()).or_default();
    for id in ids {
        toggle(&mut me.plugins, id, true);
    }
    ctx.save_data(&data)?;
    let mut paths = Vec::new();
    for id in ids {
        match data
            .plugin_sources
            .get(id)
            .filter(|url| is_git_address(url))
        {
            Some(url) => {
                run(
                    None,
                    "omarchy",
                    &["plugin", "add", url, "--enable", "--yes"],
                )?;
            }
            None => paths.push(
                ctx.home
                    .join(PLUGIN_DIR)
                    .join(id)
                    .to_string_lossy()
                    .into_owned(),
            ),
        }
    }
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["apply", "--force"];
    args.extend(paths.iter().map(|s| s.as_str()));
    run(None, "chezmoi", &args).map(|_| ())
}

/// Switches a plugin on or off in the Omarchy shell here. The state lives in shell.json, so it
/// reaches the other computers with the bar layout.
pub fn set_plugin_enabled(ctx: &Ctx, id: &str, on: bool) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    let verb = if on { "enable" } else { "disable" };
    run(None, "omarchy", &["plugin", verb, id]).map(|_| ())
}

/// Adds a plugin from its git address and switches it on, through Omarchy's own command. Under
/// the sync lock, so a background sync never saves a half-added plugin.
pub fn add_plugin(ctx: &Ctx, url: &str) -> Result<(), String> {
    forget_plugin_updates(ctx);
    let _lock = lock_wait(ctx)?;
    run(
        None,
        "omarchy",
        &["plugin", "add", url, "--enable", "--yes"],
    )
    .map(|_| ())
}

/// Updates one plugin from git, or every one that came from git, one at a time so the page
/// shows each. Returns what Omarchy said, a line per plugin.
pub fn update_plugins(ctx: &Ctx, id: Option<&str>, step: &dyn Fn(&str)) -> Result<String, String> {
    let _lock = lock_wait(ctx)?;
    forget_plugin_updates(ctx);
    let ids: Vec<String> = match id {
        Some(id) => vec![id.to_string()],
        None => ctx
            .local_plugins()
            .into_iter()
            .filter(|id| ctx.home.join(PLUGIN_DIR).join(id).join(".git").exists())
            .collect(),
    };
    let labels: Vec<String> = ids
        .iter()
        .map(|id| format!("Updating {}", ctx.plugin(id).name))
        .collect();
    if id.is_none() {
        plan(step, &labels.iter().map(String::as_str).collect::<Vec<_>>());
    }
    let mut said = Vec::new();
    for (id, label) in ids.iter().zip(&labels) {
        step(label);
        said.push(run(None, "omarchy", &["plugin", "update", id, "--yes"])?);
    }
    Ok(said.join("\n"))
}

/// Removes a plugin from this computer only; the other computers keep theirs.
pub fn remove_plugin(ctx: &Ctx, id: &str) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    run(None, "omarchy", &["plugin", "remove", id, "--yes"])?;
    // Gone from here, so the next sync doesn't count this computer as having it.
    let mut data = ctx.load_data();
    if let Some(me) = data.computers.get_mut(&ctx.host) {
        toggle(&mut me.plugins, id, false);
    }
    ctx.save_data(&data)
}
