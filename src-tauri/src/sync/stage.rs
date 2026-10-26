//! This computer's files into the sync folder: theme, background, web apps, plugins, and the commit.

use super::*;

/// Copies this computer's current files into the sync folder (not yet shared).
/// Only the background in use syncs, kept where chezmoi doesn't look (a name starting with a
/// dot); the whole backgrounds folder, which older versions synced, leaves the sync folder.
/// Returns its file name.
fn save_background(ctx: &Ctx) -> Option<String> {
    let _ = std::fs::remove_dir_all(ctx.src.join("dot_config/omarchy/backgrounds"));
    let image = ctx.current_background()?;
    let name = image.file_name()?.to_string_lossy().into_owned();
    if !is_file_name(&name) {
        return None;
    }
    let dir = ctx.src.join(".background");
    let kept = dir.join(&name);
    let same = std::fs::metadata(&kept).ok().map(|m| m.len())
        == std::fs::metadata(&image).ok().map(|m| m.len());
    if !same {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).ok()?;
        std::fs::copy(&image, &kept).ok()?;
    }
    Some(name)
}

/// Takes every theme folder but the current one out of the sync folder (older versions synced
/// them all). Each computer keeps the themes it has.
fn prune_themes(ctx: &Ctx, data: &Data) {
    let source = ctx.src.join("dot_config/omarchy/themes");
    for e in std::fs::read_dir(&source).into_iter().flatten().flatten() {
        let name = strip_attributes(&e.file_name().to_string_lossy());
        if data.theme.as_deref() != Some(name.as_str()) {
            let _ = std::fs::remove_dir_all(e.path()).or_else(|_| std::fs::remove_file(e.path()));
        }
    }
}

/// The prefixes chezmoi marks source names with (private_, symlink_, …).
const ATTRS: &[&str] = &[
    "private_",
    "readonly_",
    "empty_",
    "executable_",
    "literal_",
    "exact_",
    "symlink_",
    "create_",
    "modify_",
    "encrypted_",
];

/// A chezmoi source name without the attribute prefixes it marks files with.
pub(crate) fn strip_attributes(name: &str) -> String {
    let mut name = name;
    while let Some(rest) = ATTRS.iter().find_map(|p| name.strip_prefix(p)) {
        name = rest;
    }
    name.to_string()
}

/// Shows the synced background here when it isn't already, through Omarchy's own command.
pub(crate) fn apply_background(ctx: &Ctx, data: &Data) -> Result<(), String> {
    // The background travels with the theme's switch.
    if !data.uses_theme(&ctx.host) {
        return Ok(());
    }
    let Some(name) = data.background.as_ref().filter(|n| is_file_name(n)) else {
        return Ok(());
    };
    let desktop = here();
    if !desktop.profile().supports_themes {
        return Ok(());
    }
    let synced = ctx.src.join(".background").join(name);
    let current = ctx.current_background();
    let size = |p: &Path| std::fs::metadata(p).ok().map(|m| m.len());
    if !synced.exists()
        || current.as_ref().is_some_and(|c| {
            c.file_name().is_some_and(|n| n.to_string_lossy() == *name) && size(c) == size(&synced)
        })
    {
        return Ok(());
    }
    let dest = ctx.home.join(".local/share/omarchy-sync/background");
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let image = dest.join(name);
    std::fs::copy(&synced, &image).map_err(|e| e.to_string())?;
    desktop.set_background(&image)
}

/// Takes out of the sync folder whatever in the web app folders isn't a web app or its icon
/// (other shortcuts, caches, and ones older versions synced).
fn prune_web_apps(ctx: &Ctx, data: &Data) {
    let keep = |pick: fn(&Computer) -> &Vec<String>| -> BTreeSet<String> {
        data.computers
            .values()
            .flat_map(|c| pick(c).iter().cloned())
            .collect()
    };
    for (dir, keep) in [
        (APPS_DIR, keep(|c| &c.web_apps)),
        (ICON_DIR, keep(|c| &c.web_icons)),
    ] {
        for e in std::fs::read_dir(source_dir(ctx, dir))
            .into_iter()
            .flatten()
            .flatten()
        {
            let name = strip_attributes(&e.file_name().to_string_lossy());
            if e.path().is_file() && !keep.contains(&name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Where a folder under home is kept in the sync folder.
fn source_dir(ctx: &Ctx, dir: &str) -> PathBuf {
    ctx.src
        .join(dir.replace("/.", "/dot_").replacen('.', "dot_", 1))
}

/// The web apps and icons in the sync folder, by their names under home.
pub(crate) fn synced_web_apps(ctx: &Ctx) -> BTreeSet<(&'static str, String)> {
    [APPS_DIR, ICON_DIR]
        .into_iter()
        .flat_map(|dir| {
            std::fs::read_dir(source_dir(ctx, dir))
                .into_iter()
                .flatten()
                .flatten()
                .filter(|e| e.path().is_file())
                .map(move |e| (dir, strip_attributes(&e.file_name().to_string_lossy())))
        })
        .collect()
}

/// Deletes here the web apps and icons another computer deleted: they were in the sync folder
/// (`before`, read before taking the other computers' changes) and no longer are. Left here,
/// they'd count as new at the next sync and come back for everyone. Their files are in the
/// history.
pub(crate) fn remove_web_apps_removed_elsewhere(
    ctx: &Ctx,
    before: &BTreeSet<(&'static str, String)>,
) -> Result<(), String> {
    let mut data = ctx.load_data();
    if !data.syncs(&ctx.host, APPS_DIR) {
        return Ok(());
    }
    let after = synced_web_apps(ctx);
    let gone: BTreeSet<(&str, String)> = before.difference(&after).cloned().collect();
    if gone.is_empty() {
        return Ok(());
    }
    for (dir, name) in &gone {
        let _ = std::fs::remove_file(ctx.home.join(dir).join(name));
    }
    let listed = data.clone();
    // A clash settled in this computer's favour may have kept them in its own list.
    for c in data.computers.values_mut() {
        c.web_apps
            .retain(|a| !gone.contains(&(APPS_DIR, a.clone())));
        c.web_icons
            .retain(|i| !gone.contains(&(ICON_DIR, i.clone())));
    }
    if data != listed {
        ctx.save_data(&data)?;
        commit(ctx)?;
    }
    Ok(())
}

/// Returns how many new files (or whole folders, for an old chezmoi) it saved.
pub(crate) fn stage(ctx: &Ctx, data: &Data, step: &dyn Fn(&str)) -> Result<usize, String> {
    let mut folders = data.active_folders(&ctx.host);
    // The icons travel with the web apps.
    if folders.iter().any(|f| f == APPS_DIR) {
        folders.push(ICON_DIR.into());
    }
    let paths: Vec<String> = folders
        .iter()
        .map(|f| ctx.home.join(f))
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if paths.is_empty() {
        return Ok(0);
    }
    // Adding whole folders re-reads every file (most of a minute with big plugins), so only
    // changed files are saved again, then only new ones are added.
    step("Saving changed settings");
    let shell = shell_source(ctx);
    run(None, "chezmoi", &["re-add"])?;
    tidy(ctx, shell.as_deref());
    step("Looking for new files");
    let mut args = vec!["unmanaged", "--path-style=absolute"];
    args.extend(paths.iter().map(|s| s.as_str()));
    let new: Vec<String> = match run(None, "chezmoi", &args) {
        Ok(out) => out
            .lines()
            .filter(|l| {
                let path = Path::new(l);
                !l.is_empty() && !never_syncs(path.strip_prefix(&ctx.home).unwrap_or(path))
            })
            .map(String::from)
            .collect(),
        // Older chezmoi can't list them: add everything, slowly but completely.
        Err(_) => paths,
    };
    if new.is_empty() {
        return Ok(0);
    }
    step(&format!("Saving {}", plural(new.len(), "new file")));
    let paths: Vec<&str> = new.iter().map(|s| s.as_str()).collect();
    add(ctx, &paths)?;
    Ok(new.len())
}

/// Whether a path holds something that never syncs, whatever the exceptions say: a `.git`
/// folder, `node_modules`, a `.keep` marker or a backup file. The ignore rules name them, but
/// an exception (a shared plugin, the current theme) wins over every ignore rule in chezmoi, so
/// chezmoi reports them as new inside those folders and would add them. Works on a home path or a
/// sync folder path (`dot_git`, attribute prefixes).
fn never_syncs(path: &Path) -> bool {
    path.components().any(|c| {
        let name = strip_attributes(&c.as_os_str().to_string_lossy());
        matches!(
            name.as_str(),
            ".git" | "dot_git" | "node_modules" | ".keep" | "dot_keep"
        ) || name.ends_with(".bak")
            || name.contains(".bak.")
            || name.contains(".bak-")
            || name.ends_with(".orig")
            || name.ends_with('~')
    })
}

/// Takes out of the sync folder what never syncs, including ones saved by older versions, and
/// this computer's battery widget switch. When that switch was the only change to the shell's
/// settings since `before`, they go back to exactly `before`, so no computer shares a change.
fn tidy(ctx: &Ctx, before: Option<&[u8]>) {
    prune_ignored(&ctx.src);
    let Ok(source) = run(None, "chezmoi", &["source-path", &shell_target(ctx)]) else {
        return;
    };
    let Some(shared) = std::fs::read(&source)
        .ok()
        .and_then(|b| with_battery(&b, false))
    else {
        return;
    };
    let kept = before.filter(|b| same_json(b, &shared)).unwrap_or(&shared);
    let _ = std::fs::write(&source, kept);
}

fn shell_target(ctx: &Ctx) -> String {
    ctx.home.join(SHELL).to_string_lossy().into_owned()
}

/// The shell's settings as they are in the sync folder.
fn shell_source(ctx: &Ctx) -> Option<Vec<u8>> {
    let source = run(None, "chezmoi", &["source-path", &shell_target(ctx)]).ok()?;
    std::fs::read(source).ok()
}

/// Whether two shell settings files hold the same JSON, however written; no disabled plugins
/// is the same as an empty list (what's left once the battery widget's switch comes out).
pub(crate) fn same_json(a: &[u8], b: &[u8]) -> bool {
    let read = |b: &[u8]| {
        let mut json = serde_json::from_slice::<serde_json::Value>(b).ok()?;
        if let Some(o) = json.as_object_mut() {
            o.entry("disabledPlugins")
                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        }
        Some(json)
    };
    read(a).is_some_and(|a| Some(a) == read(b))
}

/// The shell's settings, which hold the battery widget's switch.
const SHELL: &str = ".config/omarchy/shell.json";
/// The battery widget: on where there's a battery, off where there isn't, so each computer
/// keeps its own switch and it never syncs.
const BATTERY: &str = "omarchy.battery";

/// Whether a shell.json switches the battery widget off; None when it doesn't read or doesn't
/// say.
fn battery_off(bytes: &[u8]) -> Option<bool> {
    let json: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let off = json.get("disabledPlugins")?.as_array()?;
    Some(off.iter().any(|id| id == BATTERY))
}

/// A shell.json with the battery widget switched off or on, written as the shell writes it;
/// None when it already is, or doesn't read.
pub(crate) fn with_battery(bytes: &[u8], off: bool) -> Option<Vec<u8>> {
    if battery_off(bytes).unwrap_or(false) == off {
        return None;
    }
    let mut json: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let list = json
        .as_object_mut()?
        .entry("disabledPlugins")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()))
        .as_array_mut()?;
    if off {
        list.push(BATTERY.into());
    } else {
        list.retain(|id| id != BATTERY);
    }
    let mut text = serde_json::to_vec_pretty(&json).ok()?;
    text.push(b'\n');
    Some(text)
}

/// Whether this computer runs on a battery. The lab points the folder elsewhere.
fn has_battery() -> bool {
    let dir = std::env::var_os("OMARCHY_SYNC_POWER_SUPPLY")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/sys/class/power_supply".into());
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("BAT"))
}

/// Puts the sync folder's files in place here. The battery widget keeps this computer's switch
/// (as it was, or by whether there's a battery), and the shell restarts when its settings
/// changed: it would otherwise write the ones it holds back over them.
pub(crate) fn apply_chezmoi(ctx: &Ctx) -> Result<(), String> {
    let shell = ctx.home.join(SHELL);
    let before = std::fs::read(&shell).ok();
    let off = before
        .as_deref()
        .and_then(battery_off)
        .unwrap_or_else(|| !has_battery());
    run(None, "chezmoi", &["apply", "--force"])?;
    let mut after = std::fs::read(&shell).ok();
    if after == before {
        return Ok(());
    }
    if let Some(fixed) = after.as_deref().and_then(|b| with_battery(b, off)) {
        std::fs::write(&shell, &fixed).map_err(|e| e.to_string())?;
        after = Some(fixed);
    }
    let desktop = here();
    if after != before && desktop.has_shell() {
        let _ = desktop.restart_shell();
    }
    Ok(())
}

/// Adds paths to the sync folder, then takes out whatever never syncs that came with them.
fn add(ctx: &Ctx, paths: &[&str]) -> Result<(), String> {
    let mut args = vec!["add"];
    args.extend(paths);
    run(None, "chezmoi", &args)?;
    tidy(ctx, None);
    Ok(())
}

/// Takes whatever never syncs (see `never_syncs`) out of the sync folder, for the whole-folder
/// adds that can't be filtered first.
fn prune_ignored(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = e.path();
        // The sync folder's own `.git` is its history, not a target.
        if path.is_dir() && e.file_name() == ".git" {
            continue;
        }
        if never_syncs(Path::new(&e.file_name())) {
            let _ = std::fs::remove_dir_all(&path).or_else(|_| std::fs::remove_file(&path));
        } else if path.is_dir() {
            prune_ignored(&path);
        }
    }
}

pub(crate) fn commit(ctx: &Ctx) -> Result<(), String> {
    // The author name is the computer's ID; History and "Computers" read it back from the log.
    // Set in the repo itself so pulls (which rebase, so they commit) work on computers without a git identity.
    ctx.git(&["config", "user.name", &ctx.host])?;
    ctx.git(&[
        "config",
        "user.email",
        &format!("{}@omarchy-sync", ctx.host),
    ])?;
    ctx.git(&["add", "-A"])?;
    if ctx.git(&["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(());
    }
    let staged = ctx.git(&["diff", "--cached", "--name-status"])?;
    let body = describe(&parse_changes(staged.lines()));
    ctx.git(&[
        "commit",
        "-q",
        "-m",
        &format!("Sync from {}", ctx.host),
        "-m",
        &body,
    ])
    .map(|_| ())
}

/// Commits this computer's current files (and theme) so nothing local is ever overwritten unsaved.
/// Returns this computer's new name when it had to differ from the hostname, which another
/// computer already uses.
pub(crate) fn save_local(ctx: &Ctx, step: &dyn Fn(&str)) -> Result<Option<String>, String> {
    ctx.data_readable()?;
    let mut data = ctx.load_data();
    let mut renamed = None;
    if data
        .computers
        .get(&ctx.host)
        .is_none_or(|c| c.name.is_empty())
    {
        let name = data.fresh_name(&ctx.host, &ctx.hostname);
        if name != ctx.hostname {
            renamed = Some(name.clone());
        }
        data.computers.entry(ctx.host.clone()).or_default().name = name;
    }
    // Repos made by older versions get the current ignore rules (keyed by machine ID).
    let ignore = ctx.src.join(".chezmoiignore");
    if std::fs::read_to_string(&ignore).is_ok_and(|current| current != IGNORE) {
        std::fs::write(&ignore, IGNORE).map_err(|e| e.to_string())?;
    }
    note_omarchy(&mut data, &ctx.host);
    // A computer that leaves the theme out keeps its own look without sharing it.
    if data.uses_theme(&ctx.host) {
        if let Some(t) = ctx.current_theme().filter(|t| is_file_name(t)) {
            data.theme = Some(t);
        }
        prune_themes(ctx, &data);
        if let Some(name) = save_background(ctx) {
            data.background = Some(name);
        }
    }
    // Extra repositories here, so other computers can add them for apps that come from them.
    record_repositories(&mut data);
    // Apps here that `yay` couldn't install elsewhere; they sync only once given a script.
    for app in outside_repos(&user_apps_here()) {
        data.outside.entry(app).or_insert(None);
    }
    // Without an install script such an app can't reach other computers, so it isn't synced.
    let stuck: Vec<String> = data
        .outside
        .iter()
        .filter(|(_, s)| s.is_none())
        .map(|(a, _)| a.clone())
        .collect();
    data.apps.retain(|a| !stuck.contains(a));
    let me = data.computers.entry(ctx.host.clone()).or_default();
    let (prev_plugins, prev_apps, prev_icons) = (
        std::mem::take(&mut me.plugins),
        std::mem::take(&mut me.web_apps),
        std::mem::take(&mut me.web_icons),
    );
    me.plugins = ctx.local_plugins();
    (me.web_apps, me.web_icons) = ctx.local_web_apps();
    let (now_plugins, now_apps, now_icons) = (
        me.plugins.clone(),
        me.web_apps.clone(),
        me.web_icons.clone(),
    );
    // A web app deleted here goes from every computer's list, so it leaves the sync folder
    // (see prune_web_apps) and no computer brings it back.
    if data.syncs(&ctx.host, APPS_DIR) {
        for c in data.computers.values_mut() {
            c.web_apps
                .retain(|a| now_apps.contains(a) || !prev_apps.contains(a));
            c.web_icons
                .retain(|i| now_icons.contains(i) || !prev_icons.contains(i));
        }
    }
    // A plugin added here since the last sync syncs, while plugins sync at all.
    if data.folders.iter().any(|f| f == PLUGIN_DIR) {
        for id in now_plugins.iter().filter(|id| !prev_plugins.contains(id)) {
            if !data.removed_plugins.contains_key(id) {
                toggle(&mut data.plugins, id, true);
            }
        }
    }
    // Plugins from git aren't copied: each computer adds them from git (see plugin_sources).
    for id in ctx.local_plugins() {
        if let Some(url) = ctx.plugin_source(&id) {
            data.plugin_sources.insert(id.clone(), url);
            track_plugin(ctx, &id, false)?;
        }
    }
    let known: BTreeSet<&String> = data
        .plugins
        .iter()
        .chain(data.computers.values().flat_map(|c| &c.plugins))
        .collect();
    data.plugin_sources.retain(|id, _| known.contains(id));
    prune_web_apps(ctx, &data);
    ctx.save_data(&data)?;
    stage(ctx, &data, step)?;
    commit(ctx)?;
    Ok(renamed)
}

/// Records this computer's Omarchy version.
pub(crate) fn note_omarchy(data: &mut Data, host: &str) {
    let desktop = here();
    if desktop.profile().supports_major_version_guard
        && let Some(v) = desktop.desktop_version()
    {
        let v: String = v
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(40)
            .collect();
        data.computers.entry(host.into()).or_default().omarchy = v;
    }
}

/// Starts or stops sharing one plugin. Starting adds this computer's copy (if it has one) to
/// the sync folder; stopping takes it out, and every computer keeps the copy it has.
pub fn track_plugin(ctx: &Ctx, id: &str, on: bool) -> Result<(), String> {
    let path = ctx.home.join(PLUGIN_DIR).join(id);
    let path = path.to_string_lossy();
    if on {
        if ctx.home.join(PLUGIN_DIR).join(id).exists() {
            add(ctx, &[&path])?;
        }
    } else {
        // The choice is already saved, so the ignore rules hide it from chezmoi: take its
        // folder out of the sync folder directly (it may never have been there).
        let source = ctx.src.join("dot_config/omarchy/plugins");
        for e in std::fs::read_dir(&source).into_iter().flatten().flatten() {
            if strip_attributes(&e.file_name().to_string_lossy()) == id {
                // A folder, or the symlink_ file chezmoi keeps for a linked one.
                let _ =
                    std::fs::remove_dir_all(e.path()).or_else(|_| std::fs::remove_file(e.path()));
            }
        }
    }
    Ok(())
}

/// Starts or stops syncing one folder from FOLDERS.
pub fn track(ctx: &Ctx, folder: &str, on: bool) -> Result<(), String> {
    // Stopping a folder that holds separately synced parts stops only the rest of it.
    let children: Vec<&str> = NESTED
        .iter()
        .filter(|n| n.starts_with(&format!("{folder}/")))
        .copied()
        .collect();
    if !on && !children.is_empty() {
        for e in std::fs::read_dir(ctx.home.join(folder))
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = e.path();
            let rel = format!("{folder}/{}", e.file_name().to_string_lossy());
            if !children.contains(&rel.as_str()) {
                // Fails for files never synced (ignored or new), which is the goal anyway.
                let _ = run(
                    None,
                    "chezmoi",
                    &["forget", "--force", &path.to_string_lossy()],
                );
            }
        }
        return Ok(());
    }
    let mut folders = vec![folder];
    if folder == APPS_DIR && ctx.home.join(ICON_DIR).exists() {
        folders.push(ICON_DIR);
    }
    for f in folders {
        let path = ctx.home.join(f).to_string_lossy().into_owned();
        if on {
            add(ctx, &[&path])?;
        } else {
            run(None, "chezmoi", &["forget", "--force", &path])?;
        }
    }
    Ok(())
}

/// Switches a folder in or out of the sync: the choice and the sync folder change together,
/// under one hold of the lock, so a sync can't share one without the other.
pub fn set_folder(ctx: &Ctx, folder: &str, on: bool) -> Result<Data, String> {
    let _lock = lock_wait(ctx)?;
    // Stop tracking before saving "off": once saved, the ignore rules hide it from chezmoi,
    // which then can't forget it. Starting is the other way round.
    if !on {
        track(ctx, folder, false)?;
    }
    let saved = std::fs::read(ctx.data_path()).ok();
    let data = edit_locked(ctx, |d, _| toggle(&mut d.folders, folder, on))?;
    if on && let Err(e) = track(ctx, folder, true) {
        put_back_choices(ctx, saved);
        return Err(e);
    }
    Ok(data)
}

/// Whether one computer gets a plugin (see `Data::place`), or with no computer every one (see
/// `Data::set_plugin_all`), with the sync folder following when that starts or stops sharing
/// it, under one hold of the lock.
pub fn set_plugin(ctx: &Ctx, computer: Option<&str>, id: &str, on: bool) -> Result<Data, String> {
    let _lock = lock_wait(ctx)?;
    let before = ctx.load_data().plugins.iter().any(|p| p == id);
    let saved = std::fs::read(ctx.data_path()).ok();
    // A plugin from git is added from git on each computer, never copied.
    let source = ctx.plugin_source(id).filter(|_| on);
    let data = edit_locked(ctx, |d, host| {
        // This computer's copy may be the one shared, so it counts as having what's here.
        d.computers.entry(host.into()).or_default().plugins = ctx.local_plugins();
        if let Some(url) = &source {
            d.plugin_sources.insert(id.into(), url.clone());
        }
        match computer {
            Some(c) => d.place_plugin(c, id, on),
            None => d.set_plugin_all(host, id, on),
        }
    })?;
    let after = data.plugins.iter().any(|p| p == id) && source.is_none();
    if before != after
        && let Err(e) = track_plugin(ctx, id, after)
    {
        put_back_choices(ctx, saved);
        return Err(e);
    }
    Ok(data)
}

/// Puts the choices file back as it was before a switch whose files couldn't follow.
fn put_back_choices(ctx: &Ctx, saved: Option<Vec<u8>>) {
    let _ = match saved {
        Some(bytes) => std::fs::write(ctx.data_path(), bytes),
        None => std::fs::remove_file(ctx.data_path()),
    };
}
