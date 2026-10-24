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
                source: ctx.plugin_source(id),
            })
        })
        .collect()
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
    let _lock = lock_wait(ctx)?;
    run(
        None,
        "omarchy",
        &["plugin", "add", url, "--enable", "--yes"],
    )
    .map(|_| ())
}

/// Updates one plugin from git, or every one that came from git. Returns what Omarchy said.
pub fn update_plugins(ctx: &Ctx, id: Option<&str>) -> Result<String, String> {
    let _lock = lock_wait(ctx)?;
    let mut args = vec!["plugin", "update"];
    args.extend(id);
    args.push("--yes");
    run(None, "omarchy", &args)
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
