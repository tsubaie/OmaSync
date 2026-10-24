//! What the Sync page shows.

use super::*;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Overview {
    /// Paths under home (or DATA_LABEL) changed here and not shared yet.
    pub unshared: Vec<String>,
    /// What changed in the choices (DATA_LABEL's entry), in sentences.
    pub choice_changes: Vec<String>,
    pub incoming: usize,
    pub online: bool,
    /// (computer name, last sync as unix time), this computer first.
    pub computers: Vec<(String, Option<i64>)>,
    pub apps: Apps,
    /// What the other computers' waiting updates would change here, and who made them.
    pub incoming_files: Vec<String>,
    pub incoming_from: Vec<String>,
    pub history: Vec<Entry>,
    /// Every computer's plugins, named.
    pub plugins: Vec<Plugin>,
    /// Extra repositories this computer needs for apps it should have, waiting to be added.
    pub missing_repos: Vec<MissingRepo>,
    /// Shared extra repositories this computer already has.
    pub repos_here: Vec<MissingRepo>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Apps {
    pub installed: BTreeSet<String>,
    /// Installed here by the person (not part of Omarchy), synced or not.
    pub mine: Vec<String>,
    /// Those of `mine` that came from neither Arch's repos nor the AUR.
    pub outside: Vec<String>,
    /// Where each app comes from: omarchy, arch, aur, elsewhere, or another repository's name.
    pub source: BTreeMap<String, String>,
    pub about: BTreeMap<String, String>,
    /// Each app's own website, as its package names it.
    pub website: BTreeMap<String, String>,
}

/// `just_synced`: a sync has just saved and fetched, so only the rest is looked at again (its
/// progress continues the sync's own list).
pub fn overview(
    ctx: &Ctx,
    data: &Data,
    step: &dyn Fn(&str),
    just_synced: bool,
) -> Result<Overview, String> {
    // Staging and git status would collide with a background sync mid-commit.
    let _lock = lock_wait(ctx)?;
    if !just_synced {
        plan(
            step,
            &[
                "Saving changed settings",
                "Looking for new files",
                "Checking GitHub for your other computers",
                "Checking your apps and plugins",
            ],
        );
    }
    let online = just_synced || {
        stage(ctx, data, step)?;
        step("Checking GitHub for your other computers");
        git_progress(ctx, &["fetch", "--progress"], step).is_ok()
    };
    let incoming = ctx
        .git(&["rev-list", "--count", "HEAD..@{u}"])
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    let status = ctx.git(&["status", "--porcelain", "-z", "--untracked-files=all"])?;
    let log = ctx.git(&["log", "--format=%an%x09%ct"]).unwrap_or_default();
    let incoming_files = ctx
        .git(&["diff", "--name-only", "HEAD...@{u}"])
        .unwrap_or_default();
    let mut incoming_from: Vec<String> = Vec::new();
    for name in ctx
        .git(&["log", "--format=%an", "HEAD..@{u}"])
        .unwrap_or_default()
        .lines()
    {
        if !incoming_from.iter().any(|n| n == name) {
            incoming_from.push(name.to_string());
        }
    }
    step("Checking your apps and plugins");
    let apps = apps(data);
    let here = repos_here();
    let mut plugins = all_plugins(ctx, data);
    let updates = plugin_updates_today(ctx);
    for p in &mut plugins {
        p.update = updates.get(&p.id).copied();
    }
    let mut unshared: Vec<String> = parse_status(&status)
        .iter()
        .filter(|p| !p.starts_with(".chezmoiignore"))
        .map(|p| source_to_target(p))
        .collect();
    for change in look_changes(ctx, data) {
        if !unshared.contains(&change) {
            unshared.push(change);
        }
    }
    Ok(Overview {
        unshared,
        choice_changes: ctx.choice_changes(),
        incoming,
        online,
        computers: computers(&parse_log(&log), data, &ctx.host),
        incoming_files: incoming_files
            .lines()
            .filter(|f| !f.starts_with(".chezmoiignore"))
            .map(source_to_target)
            .collect(),
        incoming_from,
        history: history(ctx)
            .into_iter()
            .map(|e| Entry {
                computer: data.resolve(&e.computer),
                ..e
            })
            .collect(),
        plugins,
        missing_repos: missing_repos(data, &ctx.host, &apps.installed, &here),
        repos_here: repos_with_apps(data, &here),
        apps,
    })
}

/// Maps paths inside the sync folder to paths under home.
/// The theme and background in use here when they differ from the shared ones. Neither shows
/// in git until a sync records it, so a change made in Omarchy is listed from here.
pub(crate) fn look_changes(ctx: &Ctx, data: &Data) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(theme) = ctx.current_theme().filter(|t| is_file_name(t))
        && data.uses_theme(&ctx.host)
        && data.theme.as_ref() != Some(&theme)
    {
        out.push(format!(".config/omarchy/themes/{theme}"));
    }
    if let Some(name) = ctx
        .current_background()
        .and_then(|p| Some(p.file_name()?.to_string_lossy().into_owned()))
        .filter(|n| is_file_name(n))
        && data.background.as_ref() != Some(&name)
    {
        out.push(format!(".background/{name}"));
    }
    out
}

/// Paths from `git status --porcelain -z`.
pub fn parse_status(s: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut entries = s.split('\0');
    while let Some(e) = entries.next() {
        if e.len() > 3 {
            paths.push(e[3..].to_string());
            // A rename or copy ("R  new", then the old name) is one change, not two.
            if e.starts_with(['R', 'C']) {
                entries.next();
            }
        }
    }
    paths
}

/// Latest commit time per author from `git log --format=%an%x09%ct` (newest first).
pub fn parse_log(s: &str) -> BTreeMap<String, i64> {
    let mut last = BTreeMap::new();
    for line in s.lines() {
        if let Some((name, t)) = line.split_once('\t')
            && let Ok(t) = t.parse()
        {
            last.entry(name.to_string()).or_insert(t);
        }
    }
    last
}

/// Every known computer by ID with its latest sync, this computer first. Commits made under an
/// older name count for the computer that name now belongs to.
pub(crate) fn computers(
    last: &BTreeMap<String, i64>,
    data: &Data,
    host: &str,
) -> Vec<(String, Option<i64>)> {
    let mut latest: BTreeMap<String, Option<i64>> =
        data.computers.keys().map(|id| (id.clone(), None)).collect();
    for (author, t) in last {
        let seen = latest.entry(data.resolve(author)).or_default();
        *seen = (*seen).max(Some(*t));
    }
    let mut list: Vec<_> = latest.into_iter().collect();
    list.sort_by_key(|(id, _)| id != host);
    list
}

/// Name -> description from `pacman -Qi` output.
/// One `field` of each package in `pacman -Qi` output, by package name.
pub fn parse_info(s: &str, field: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut name = None;
    for line in s.lines() {
        if let Some((k, v)) = line.split_once(" : ") {
            let k = k.trim();
            if k == "Name" {
                name = Some(v.trim().to_string());
            } else if k == field
                && let Some(n) = name.take()
            {
                out.insert(n, v.trim().to_string());
            }
        }
    }
    out
}

/// Plain name for a path under home, from FOLDERS.
pub fn label(path: &str) -> &str {
    if path.starts_with(".background/") {
        return "Background";
    }
    FOLDERS
        .iter()
        .filter(|(f, _, _)| path == *f || path.starts_with(&format!("{f}/")))
        .max_by_key(|(f, _, _)| f.len())
        .map_or(path, |(_, l, _)| l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsers() {
        assert_eq!(
            parse_status(" M dot_config/hypr/a.lua\0?? .chezmoidata.yaml\0"),
            vec!["dot_config/hypr/a.lua", ".chezmoidata.yaml"]
        );
        let log = parse_log("laptop\t300\ndesktop\t200\nlaptop\t100\n");
        assert_eq!(log["laptop"], 300);
        assert_eq!(log["desktop"], 200);
        let out = "Name            : brave-bin\nVersion         : 1\nDescription     : Web browser\nURL             : https://brave.com\n\nName            : yazi\nDescription     : File manager\n";
        let info = parse_info(out, "Description");
        assert_eq!(info["brave-bin"], "Web browser");
        assert_eq!(info["yazi"], "File manager");
        let sites = parse_info(out, "URL");
        assert_eq!(sites["brave-bin"], "https://brave.com");
        assert!(!sites.contains_key("yazi"));
    }

    #[test]
    fn a_rename_is_one_change() {
        let status = "R  dot_config/hypr/new.lua\0dot_config/hypr/old.lua\0 M dot_config/omarchy/private_shell.json\0";
        assert_eq!(
            parse_status(status),
            vec![
                "dot_config/hypr/new.lua".to_string(),
                "dot_config/omarchy/private_shell.json".to_string()
            ]
        );
    }

    #[test]
    fn labels_and_time() {
        assert_eq!(label(".config/hypr/bindings.lua"), "Keyboard shortcuts");
        assert_eq!(label(".config/hyprx/a"), ".config/hyprx/a");
        assert_eq!(label(".config/hypr/hyprland.lua"), "Other window settings");
        assert_eq!(plural(1, "app"), "1 app");
        assert_eq!(plural(3, "app"), "3 apps");
    }
}
