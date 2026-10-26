//! Plugins as Omarchy manages them: every one it knows here, on and off, adding one from git,
//! updating and removing. The app is a window over `omarchy plugin`.

use super::*;

/// Every plugin Omarchy knows on this computer, from its catalog (names, descriptions) and its
/// list (on or off), except the app's own bar indicator, which Settings manages.
fn omarchy_plugins(ctx: &Ctx) -> Vec<Plugin> {
    let json = |args: &[&str]| -> Vec<serde_json::Value> {
        here()
            .plugin(args)
            .ok()
            .and_then(|out| serde_json::from_str::<serde_json::Value>(&out).ok())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
    };
    let state: BTreeMap<String, (bool, bool)> = json(&["list", "--json"])
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
    json(&["catalog"])
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

/// Whether a plugin's copy here has changes nobody committed (untracked files don't count:
/// Omarchy's update goes over those).
fn edited(dir: &Path) -> bool {
    dir.join(".git").exists()
        && run(
            Some(dir),
            "git",
            &["status", "--porcelain", "--untracked-files=no"],
        )
        .is_ok_and(|out| !out.is_empty())
}

/// How each plugin from git compares with its source: edited here, or newer at the source
/// (the source's HEAD isn't among this copy's commits), or current (it is, even when this copy
/// is ahead). One short question per plugin, all at once.
pub(crate) fn plugin_updates(ctx: &Ctx) -> BTreeMap<String, PluginUpdate> {
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
                    if edited(dir) {
                        return Some((id.clone(), PluginUpdate::Edited));
                    }
                    let here = run(Some(dir), "git", &["rev-parse", "HEAD"]).ok()?;
                    // A source that doesn't answer gets 20 seconds, not the whole look.
                    let there = run(
                        Some(dir),
                        "timeout",
                        &["20", "git", "ls-remote", "origin", "HEAD"],
                    )
                    .ok()?;
                    let there = there.split_whitespace().next()?;
                    let has = there == here
                        || run(
                            Some(dir),
                            "git",
                            &["merge-base", "--is-ancestor", there, "HEAD"],
                        )
                        .is_ok();
                    let state = if has {
                        PluginUpdate::Current
                    } else {
                        PluginUpdate::Newer
                    };
                    Some((id.clone(), state))
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
    updates: BTreeMap<String, PluginUpdate>,
}

fn checked_file(ctx: &Ctx) -> PathBuf {
    ctx.state_dir().join("omarchy-sync/plugin-updates.json")
}

/// `plugin_updates`, asked at most once a day: the app looks far more often than plugin makers
/// publish. An answer that came back empty (offline) isn't kept, so the next look asks again;
/// updating or adding a plugin forgets the day's answer (see `forget_plugin_updates`). An answer
/// kept by an older version doesn't read, so it's asked again.
pub(crate) fn plugin_updates_today(ctx: &Ctx) -> BTreeMap<String, PluginUpdate> {
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

/// What the Plugins page lists: everything Omarchy knows here, plus the synced plugins this
/// computer doesn't have, by name. Another computer's unsynced ones stay its own business.
pub(crate) fn all_plugins(ctx: &Ctx, data: &Data) -> Vec<Plugin> {
    let mut plugins = omarchy_plugins(ctx);
    let mut ids: BTreeSet<String> = ctx.local_plugins().into_iter().collect();
    ids.extend(data.plugins.iter().cloned());
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
    install_plugins_locked(ctx, ids)
}

/// `install_plugins` for a caller already holding the lock (a sync). This computer counts as
/// having a plugin only once it's here: one from git after Omarchy added it, a copied one after
/// its files were written out. One that fails stays waiting, and the rest still arrive.
pub(crate) fn install_plugins_locked(ctx: &Ctx, ids: &[String]) -> Result<(), String> {
    let mut data = ctx.load_data();
    let mut errors = Vec::new();
    let mut copied = Vec::new();
    for id in ids {
        match data
            .plugin_sources
            .get(id)
            .filter(|url| is_git_address(url))
        {
            Some(url) => match here().plugin(&["add", url, "--enable", "--yes"]) {
                Ok(_) => {
                    let me = data.computers.entry(ctx.host.clone()).or_default();
                    toggle(&mut me.plugins, id, true);
                    ctx.save_data(&data)?;
                }
                Err(e) => errors.push(format!("{}: {e}", ctx.plugin(id).name)),
            },
            None => copied.push(id.clone()),
        }
    }
    if !copied.is_empty() {
        // Counted as here first: the ignore rules only let a plugin through to a computer
        // that has it.
        let me = data.computers.entry(ctx.host.clone()).or_default();
        for id in &copied {
            toggle(&mut me.plugins, id, true);
        }
        ctx.save_data(&data)?;
        let paths: Vec<String> = copied
            .iter()
            .map(|id| {
                ctx.home
                    .join(PLUGIN_DIR)
                    .join(id)
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        let mut args = vec!["apply", "--force"];
        args.extend(paths.iter().map(|s| s.as_str()));
        if let Err(e) = run(None, "chezmoi", &args) {
            let me = data.computers.entry(ctx.host.clone()).or_default();
            for id in &copied {
                toggle(&mut me.plugins, id, false);
            }
            ctx.save_data(&data)?;
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("\n"))
    }
}

/// Switches a plugin on or off in the Omarchy shell here. The state lives in shell.json, so it
/// reaches the other computers with the bar layout.
pub fn set_plugin_enabled(ctx: &Ctx, id: &str, on: bool) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    let verb = if on { "enable" } else { "disable" };
    here().plugin(&[verb, id]).map(|_| ())
}

/// Adds a plugin from its git address and switches it on, through Omarchy's own command. Under
/// the sync lock, so a background sync never saves a half-added plugin. It syncs straight away
/// while plugins sync, added from git on the other computers.
pub fn add_plugin(ctx: &Ctx, url: &str) -> Result<(), String> {
    forget_plugin_updates(ctx);
    let _lock = lock_wait(ctx)?;
    let before = ctx.local_plugins();
    here().plugin(&["add", url, "--enable", "--yes"])?;
    if !is_set_up(ctx) {
        return Ok(());
    }
    let new: Vec<String> = ctx
        .local_plugins()
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect();
    edit_locked(ctx, |d, host| {
        let syncs = d.folders.iter().any(|f| f == PLUGIN_DIR);
        for id in &new {
            toggle(
                &mut d.computers.entry(host.into()).or_default().plugins,
                id,
                true,
            );
            if let Some(source) = ctx.plugin_source(id) {
                d.plugin_sources.insert(id.clone(), source);
            }
            if syncs && !d.removed_plugins.contains_key(id) {
                toggle(&mut d.plugins, id, true);
            }
        }
    })
    .map(|_| ())
}

/// What updating plugins came to, in sentences; `warn` when one was skipped or failed.
#[derive(Debug, Clone, Serialize)]
pub struct PluginsUpdated {
    pub text: String,
    pub warn: bool,
}

/// Updates one plugin from git, or every one that came from git, one at a time so the page
/// shows each. One changed on this computer is skipped (Omarchy won't update over changes), and
/// one that fails doesn't stop the rest; the summary says what happened to each.
pub fn update_plugins(
    ctx: &Ctx,
    id: Option<&str>,
    step: &dyn Fn(&str),
) -> Result<PluginsUpdated, String> {
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
    let names: Vec<String> = ids.iter().map(|id| ctx.plugin(id).name).collect();
    let labels: Vec<String> = names.iter().map(|n| format!("Updating {n}")).collect();
    if id.is_none() {
        plan(step, &labels.iter().map(String::as_str).collect::<Vec<_>>());
    }
    let (mut updated, mut current) = (0, 0);
    let mut problems = Vec::new();
    for ((id, name), label) in ids.iter().zip(&names).zip(&labels) {
        step(label);
        if edited(&ctx.home.join(PLUGIN_DIR).join(id)) {
            problems.push(format!(
                "{name} has changes on this computer, so it wasn't updated."
            ));
            continue;
        }
        match here().plugin(&["update", id, "--yes"]) {
            Ok(out) if out.lines().any(|l| l.starts_with("Updated")) => updated += 1,
            Ok(_) => current += 1,
            Err(e) => problems.push(format!(
                "{name} couldn't be updated: {}.",
                update_error(&e).trim_end_matches('.')
            )),
        }
    }
    let summary = match (updated, current) {
        (0, 0) => None,
        (0, _) if ids.len() == 1 => Some(format!("{} is up to date.", names[0])),
        (0, _) => Some("Your plugins are up to date.".to_string()),
        (n, 0) => Some(format!("Updated {}.", plural(n, "plugin"))),
        (n, _) => Some(format!(
            "Updated {}; the rest were up to date.",
            plural(n, "plugin")
        )),
    };
    Ok(PluginsUpdated {
        warn: !problems.is_empty(),
        text: summary
            .into_iter()
            .chain(problems)
            .collect::<Vec<_>>()
            .join(" "),
    })
}

/// Omarchy's reason, without the command in front ("cannot fast-forward 'x'; …").
fn update_error(err: &str) -> String {
    let last = err.lines().last().unwrap_or(err);
    match last.rsplit_once("omarchy-plugin-update: ") {
        Some((_, why)) => why.trim().to_string(),
        None => friendly(err),
    }
}

/// Removes a plugin here now, and from every computer that syncs it at its next sync: its Sync
/// switch goes off everywhere. A computer that switched it off for itself keeps its copy.
pub fn remove_plugin(ctx: &Ctx, id: &str) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    here().plugin(&["remove", id, "--yes"])?;
    let saved = ctx.load_data();
    let synced = saved.plugins.iter().any(|p| p == id);
    edit_locked(ctx, |d, host| {
        if synced {
            let others: Vec<String> = d
                .computers
                .keys()
                .filter(|c| *c != host && d.wants_plugin(c, id))
                .filter(|c| d.computers[*c].plugins.iter().any(|p| p == id))
                .cloned()
                .collect();
            if !others.is_empty() {
                d.removed_plugins.insert(id.to_string(), others);
            }
            toggle(&mut d.plugins, id, false);
            for c in d.computers.values_mut() {
                toggle(&mut c.skip_plugins, id, false);
            }
        }
        // Gone from here, so the next sync doesn't count this computer as having it.
        toggle(
            &mut d.computers.entry(host.into()).or_default().plugins,
            id,
            false,
        );
    })?;
    // Its files leave the sync folder too (no-op for one from git, which never entered it).
    if synced {
        track_plugin(ctx, id, false)?;
    }
    Ok(())
}

/// At a sync, once the other computers' choices are in: removes the plugins another computer
/// removed while this one synced them, as `remove_plugin` asked. Returns a note for the person.
pub(crate) fn remove_plugins_removed_elsewhere(ctx: &Ctx) -> Result<Option<String>, String> {
    let data = ctx.load_data();
    let ids: Vec<String> = data
        .removed_plugins
        .iter()
        .filter(|(_, computers)| computers.contains(&ctx.host))
        .map(|(id, _)| id.clone())
        .filter(|id| is_plugin_id(id))
        .collect();
    if ids.is_empty() {
        return Ok(None);
    }
    // A copy here may hold the person's own changes (one from git never syncs its files), so
    // every copy is saved before it goes.
    let copies: Vec<String> = ids
        .iter()
        .filter(|id| ctx.home.join(PLUGIN_DIR).join(id).exists())
        .map(|id| format!("{PLUGIN_DIR}/{id}"))
        .collect();
    if !copies.is_empty() {
        backup_now(ctx, &copies, "before-removing-plugins")?;
    }
    let mut gone = Vec::new();
    let mut failed = Vec::new();
    for id in &ids {
        let name = ctx.plugin(id).name;
        if !ctx.home.join(PLUGIN_DIR).join(id).exists() {
            continue;
        }
        match here().plugin(&["remove", id, "--yes"]) {
            Ok(_) => gone.push(name),
            Err(_) => failed.push(name),
        }
    }
    edit_locked(ctx, |d, host| {
        for id in &ids {
            if let Some(computers) = d.removed_plugins.get_mut(id) {
                toggle(computers, host, false);
                if computers.is_empty() {
                    d.removed_plugins.remove(id);
                }
            }
            if !ctx.home.join(PLUGIN_DIR).join(id).exists() {
                toggle(
                    &mut d.computers.entry(host.into()).or_default().plugins,
                    id,
                    false,
                );
            }
        }
    })?;
    ctx.git(&["commit", "-q", "-am", &format!("Sync from {}", ctx.host)])?;
    let mut said = Vec::new();
    if !gone.is_empty() {
        said.push(format!(
            "Removed {} here, as it was removed on another computer. A copy was saved first.",
            gone.join(", ")
        ));
    }
    if !failed.is_empty() {
        said.push(format!(
            "{} couldn't be removed here, so it stays as this computer's own.",
            failed.join(", ")
        ));
    }
    Ok((!said.is_empty()).then(|| said.join(" ")))
}
