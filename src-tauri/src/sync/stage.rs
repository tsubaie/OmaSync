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
    let Some(name) = data.background.as_ref().filter(|n| is_file_name(n)) else {
        return Ok(());
    };
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
    run(
        None,
        "omarchy",
        &["theme", "bg", "set", &image.to_string_lossy()],
    )
    .map(|_| ())
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
        let source = ctx
            .src
            .join(dir.replace("/.", "/dot_").replacen('.', "dot_", 1));
        for e in std::fs::read_dir(&source).into_iter().flatten().flatten() {
            let name = strip_attributes(&e.file_name().to_string_lossy());
            if e.path().is_file() && !keep.contains(&name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
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
    run(None, "chezmoi", &["re-add"])?;
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
/// folder, `node_modules` or a backup file. The ignore rules name them, but an exception (a
/// shared plugin, the current theme) wins over every ignore rule in chezmoi, so chezmoi
/// reports them as new inside those folders and would add them. Works on a home path or a
/// sync folder path (`dot_git`, attribute prefixes).
fn never_syncs(path: &Path) -> bool {
    path.components().any(|c| {
        let name = strip_attributes(&c.as_os_str().to_string_lossy());
        matches!(name.as_str(), ".git" | "dot_git" | "node_modules") || name.contains(".bak.")
    })
}

/// Adds paths to the sync folder, then takes out whatever never syncs that came with them.
fn add(ctx: &Ctx, paths: &[&str]) -> Result<(), String> {
    let mut args = vec!["add"];
    args.extend(paths);
    run(None, "chezmoi", &args)?;
    prune_ignored(&ctx.src);
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
    if let Some(t) = ctx.current_theme().filter(|t| is_file_name(t)) {
        data.theme = Some(t);
    }
    prune_themes(ctx, &data);
    if let Some(name) = save_background(ctx) {
        data.background = Some(name);
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
    me.plugins = ctx.local_plugins();
    (me.web_apps, me.web_icons) = ctx.local_web_apps();
    // Plugins from git aren't copied: each computer adds them from git (see plugin_sources).
    for id in ctx.local_plugins() {
        if let Some(url) = ctx.plugin_source(&id) {
            data.plugin_sources.insert(id.clone(), url);
            track_plugin(ctx, &id, false)?;
        }
    }
    prune_web_apps(ctx, &data);
    ctx.save_data(&data)?;
    stage(ctx, &data, step)?;
    commit(ctx)?;
    Ok(renamed)
}

/// Records this computer's Omarchy version.
pub(crate) fn note_omarchy(data: &mut Data, host: &str) {
    if let Ok(v) = run(None, "omarchy", &["version"]) {
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
