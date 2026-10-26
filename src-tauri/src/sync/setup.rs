//! First-time setup: what's missing, the first computer's sync space, another computer joining.

use super::*;

pub const REPO: &str = "omarchy-sync";

#[derive(Clone, Debug, Serialize)]
pub struct SetupState {
    /// Tools that must be installed before anything else; the package adapter names their packages.
    pub missing: Vec<&'static str>,
    pub signed_in: bool,
    /// Some(login) once signed in; `found` says whether that account already has synced settings.
    pub login: Option<String>,
    pub found: bool,
    pub ready: bool,
    /// Every folder setup can offer, and whether a first computer syncs it unless told otherwise.
    pub folders: Vec<SetupFolder>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SetupFolder {
    pub path: &'static str,
    /// On by default here: what `seed` picks when nobody chooses.
    pub default: bool,
}

pub fn probe(ctx: &Ctx) -> SetupState {
    let missing: Vec<&'static str> = ["chezmoi", "gh", "git"]
        .into_iter()
        .filter(|bin| !has(bin))
        .collect();
    let mut s = SetupState {
        ready: false,
        signed_in: false,
        login: None,
        found: false,
        missing,
        folders: Vec::new(),
    };
    if !s.missing.is_empty() {
        return s;
    }
    if is_set_up(ctx) {
        s.ready = true;
        return s;
    }
    let defaults = first_folders(ctx, None);
    s.folders = folders_for_profile(&here().profile())
        .into_iter()
        .map(|(path, _, _)| SetupFolder {
            path,
            default: defaults.contains(&path),
        })
        .collect();
    s.signed_in = run(None, "gh", &["auth", "status"]).is_ok();
    if s.signed_in {
        s.login = run(None, "gh", &["api", "user", "--jq", ".login"]).ok();
        if let Some(login) = &s.login {
            s.found = run(
                None,
                "gh",
                &["repo", "view", &format!("{login}/{REPO}"), "--json", "name"],
            )
            .is_ok();
        }
    }
    s
}

/// Set up means the GitHub copy exists. A local folder alone is a setup that was interrupted,
/// and pressing Start syncing again finishes it.
pub fn is_set_up(ctx: &Ctx) -> bool {
    ctx.src.join(".git").exists() && ctx.git(&["remote", "get-url", "origin"]).is_ok()
}

pub fn install_helpers(need: &[&str]) -> Result<(), String> {
    here().install_helpers(need)
}

pub fn sign_in() -> Result<(), String> {
    Command::new("xdg-terminal-exec")
        .args([
            "gh",
            "auth",
            "login",
            "--web",
            "--git-protocol",
            "https",
            "--hostname",
            "github.com",
        ])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// First computer: save its settings into a new private GitHub repo. Holds the sync lock
/// throughout, so the background timer can't sync a half-made sync folder. `folders`: the ones
/// the person chose (None: the defaults).
pub fn start(ctx: &Ctx, folders: Option<&[String]>, step: &dyn Fn(&str)) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    plan(
        step,
        &[
            "Getting ready",
            "Saving this computer's settings",
            "Creating your private sync space on GitHub",
        ],
    );
    step("Getting ready");
    run(None, "gh", &["auth", "setup-git"])?;
    step("Saving this computer's settings");
    seed(ctx, folders)?;
    step("Creating your private sync space on GitHub");
    let src = ctx.src.to_string_lossy();
    run(
        None,
        "gh",
        &[
            "repo",
            "create",
            REPO,
            "--private",
            "--source",
            &src,
            "--remote",
            "origin",
            "--push",
        ],
    )?;
    Ok(())
}

/// The folders a first computer starts with: those chosen (known and here), or by default the
/// default ones that are here.
fn first_folders(ctx: &Ctx, chosen: Option<&[String]>) -> Vec<&'static str> {
    // An app's settings folder counts only when the app is installed: Omarchy ships settings
    // for terminals nobody may use.
    let installed = installed();
    folders_for_profile(&here().profile())
        .into_iter()
        .filter(|(p, _, _)| ctx.home.join(p).exists())
        .filter(|(p, _, on)| match chosen {
            Some(list) => list.iter().any(|f| f == p),
            None => *on && folder_app(p).is_none_or(|app| installed.contains(app)),
        })
        .map(|(p, _, _)| p)
        .collect()
}

/// Creates the local sync folder from this computer's settings and commits it.
pub(crate) fn seed(ctx: &Ctx, chosen: Option<&[String]>) -> Result<(), String> {
    run(None, "chezmoi", &["init"])?;
    std::fs::write(ctx.src.join(".chezmoiignore"), IGNORE).map_err(|e| e.to_string())?;
    let mut data = Data {
        folders: first_folders(ctx, chosen)
            .into_iter()
            .map(String::from)
            .collect(),
        theme: ctx.current_theme(),
        ..Default::default()
    };
    data.computers.entry(ctx.host.clone()).or_default().name = ctx.hostname.clone();
    ctx.save_data(&data)?;
    stage(ctx, &data, &|_| {})?;
    commit(ctx)
}

/// Another computer already syncs: back up this one, then take the synced settings. Holds the
/// sync lock throughout, like `start`. `keep`: folders this computer keeps its own version of.
pub fn join(
    ctx: &Ctx,
    login: &str,
    keep: &[String],
    step: &dyn Fn(&str),
) -> Result<PathBuf, String> {
    let _lock = lock_wait(ctx)?;
    plan(
        step,
        &[
            "Getting ready",
            "Downloading your synced settings",
            "Backing up this computer's current settings",
            "Updating this computer",
        ],
    );
    step("Getting ready");
    run(None, "gh", &["auth", "setup-git"])?;
    adopt(
        ctx,
        &format!("https://github.com/{login}/{REPO}.git"),
        keep,
        step,
    )
}

pub(crate) fn adopt(
    ctx: &Ctx,
    url: &str,
    keep: &[String],
    step: &dyn Fn(&str),
) -> Result<PathBuf, String> {
    step("Downloading your synced settings");
    run(None, "chezmoi", &["init", url])?;
    let mut data = ctx.load_data();
    // Recorded before the apply below, whose ignore rules then leave these folders alone.
    for folder in keep {
        data.set_folder_here(&ctx.host, folder, false);
    }
    // Joining with an older Omarchy than the others: its own settings stay as they are.
    note_omarchy(&mut data, &ctx.host);
    let paused = data.should_pause(&ctx.host);
    data.computers.entry(ctx.host.clone()).or_default().paused = paused;
    ctx.save_data(&data)?;
    step("Backing up this computer's current settings");
    let backup = backup_now(
        ctx,
        &with_current_look(ctx, &data.folders),
        "before-first-sync",
    )?;
    step("Updating this computer");
    run(None, "chezmoi", &["apply", "--force"])?;
    Ok(backup)
}

/// `folders` plus this computer's theme and background, which the first sync replaces with the
/// synced ones: what's current (the links in Omarchy's state folder and, from older versions,
/// in ~/.config/omarchy/current), the backgrounds folder, and the image shown when it's
/// elsewhere in home.
fn with_current_look(ctx: &Ctx, folders: &[String]) -> Vec<String> {
    // The image's path is resolved, so home is too.
    let home = std::fs::canonicalize(&ctx.home).unwrap_or_else(|_| ctx.home.clone());
    let image = ctx
        .current_background()
        .and_then(|p| Some(p.strip_prefix(&home).ok()?.to_string_lossy().into_owned()));
    let mut all = folders.to_vec();
    for path in [
        ".local/state/omarchy/current",
        ".config/omarchy/current",
        ".config/omarchy/backgrounds",
    ]
    .map(String::from)
    .into_iter()
    .chain(image)
    {
        if !all
            .iter()
            .any(|f| path == *f || path.starts_with(&format!("{f}/")))
        {
            all.push(path);
        }
    }
    all
}

/// `backup` named `{prefix}-{now}`, never over an earlier one: two in the same second (an
/// undo, then a sync) each keep their own file.
pub(crate) fn backup_now(ctx: &Ctx, folders: &[String], prefix: &str) -> Result<PathBuf, String> {
    let dir = ctx.backups();
    let stamp = format!("{prefix}-{}", now());
    let name = std::iter::once(stamp.clone())
        .chain((2..).map(|n| format!("{stamp}-{n}")))
        .find(|n| !dir.join(format!("{n}.tar.gz")).exists())
        .unwrap_or(stamp);
    backup(ctx, folders, &name)
}

pub(crate) fn backup(ctx: &Ctx, folders: &[String], name: &str) -> Result<PathBuf, String> {
    let dir = ctx.backups();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{name}.tar.gz"));
    let existing: Vec<&str> = folders
        .iter()
        .map(|f| f.as_str())
        .filter(|f| ctx.home.join(f).exists())
        .collect();
    if !existing.is_empty() {
        let (f, h) = (file.to_string_lossy(), ctx.home.to_string_lossy());
        let mut args = vec!["-czf", &f, "-C", &h];
        args.extend(existing);
        run(None, "tar", &args)?;
    }
    Ok(file)
}

/// A safety backup, as History lists it.
#[derive(Clone, Debug, Serialize)]
pub struct BackupEntry {
    /// Its file name in the backups folder, which `restore_backup` takes.
    pub name: String,
    /// What it was made before, in words ("Before syncing").
    pub label: String,
    /// When it was made, as unix time.
    pub at: u64,
    /// The folders and files in it, as paths under home.
    pub folders: Vec<String>,
}

/// A backup's file name: a plain `.tar.gz` name, never a path.
pub fn is_backup_name(name: &str) -> bool {
    name.len() > ".tar.gz".len()
        && name.ends_with(".tar.gz")
        && !name.contains(['/', '\\', '\0'])
        && !name.contains("..")
}

/// A path inside a backup that stays under home when unpacked: relative, and never `..`.
fn is_safe_member(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && Path::new(path).components().all(|c| {
            matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

/// The outermost folders and files in a backup, after checking every path in it is safe.
fn backup_contents(file: &Path) -> Result<Vec<String>, String> {
    let listing = run(None, "tar", &["-tzf", &file.to_string_lossy()])?;
    let members: BTreeSet<&str> = listing
        .lines()
        .map(|l| l.trim_end_matches('/'))
        .filter(|l| !l.is_empty() && *l != ".")
        .collect();
    if let Some(bad) = members.iter().find(|m| !is_safe_member(m)) {
        return Err(format!("the backup holds an unsafe path: {bad}"));
    }
    Ok(members
        .iter()
        .filter(|m| {
            m.match_indices('/')
                .all(|(i, _)| !members.contains(&m[..i]))
        })
        .map(|m| m.to_string())
        .collect())
}

/// What `backup_now` named it before: "before-syncing-1712345678-2" is "Before syncing".
fn backup_label(name: &str) -> String {
    let stem = name.trim_end_matches(".tar.gz");
    let words: Vec<&str> = stem
        .split('-')
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()))
        .collect();
    let text = words.join(" ").replace("omarchy", "Omarchy");
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

// shortcut: only the newest 20 are read (each one is unpacked to list it); page them if people need older ones.
const BACKUPS_SHOWN: usize = 20;

/// The safety backups, newest first. One that can't be read or isn't safe isn't listed.
pub fn list_backups(ctx: &Ctx) -> Vec<BackupEntry> {
    let mut found: Vec<(u64, String)> = std::fs::read_dir(ctx.backups())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let at = e
                .metadata()
                .ok()?
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_secs();
            is_backup_name(&name).then_some((at, name))
        })
        .collect();
    found.sort_by(|a, b| b.cmp(a));
    found
        .into_iter()
        .take(BACKUPS_SHOWN)
        .filter_map(|(at, name)| {
            let folders = backup_contents(&ctx.backups().join(&name)).ok()?;
            Some(BackupEntry {
                label: backup_label(&name),
                name,
                at,
                folders,
            })
        })
        .collect()
}

/// Puts a safety backup's files back in home, after backing up what they replace. Inside synced
/// folders they're saved to the sync folder, to share at the next sync.
pub fn restore_backup(ctx: &Ctx, name: &str) -> Result<(), String> {
    if !is_backup_name(name) {
        return Err(format!("not a backup: {name}"));
    }
    let _lock = lock_wait(ctx)?;
    let file = ctx.backups().join(name);
    if !file.is_file() {
        return Err(format!("no backup called {name}"));
    }
    let contents = backup_contents(&file)?;
    backup_now(ctx, &contents, "before-restoring")?;
    let (f, h) = (file.to_string_lossy(), ctx.home.to_string_lossy());
    run(None, "tar", &["-xzf", &f, "-C", &h])?;
    if is_set_up(ctx) {
        stage(ctx, &ctx.load_data(), &|_| {})?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backups_made_in_the_same_second_each_keep_their_own_file() {
        let home = TempDir::new("backups").unwrap();
        let ctx = Ctx {
            home: home.path().to_path_buf(),
            src: home.path().join("src"),
            host: "h".into(),
            hostname: "h".into(),
        };
        let folders = vec![".config/hypr".to_string()];
        let file = ctx.home.join(".config/hypr/bindings.lua");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        // Fast enough to share a second, at least the first two.
        let made: Vec<PathBuf> = ["first", "second", "third"]
            .iter()
            .map(|content| {
                std::fs::write(&file, content).unwrap();
                backup_now(&ctx, &folders, "before-syncing").unwrap()
            })
            .collect();
        for (path, content) in made.iter().zip(["first", "second", "third"]) {
            let f = path.to_string_lossy();
            let kept = run(None, "tar", &["-xOzf", &f, ".config/hypr/bindings.lua"]);
            assert_eq!(kept.as_deref(), Ok(content), "{path:?}");
        }
    }

    #[test]
    fn backup_names_and_paths_never_leave_their_folders() {
        assert!(is_backup_name("before-syncing-1712345678.tar.gz"));
        for bad in [
            "../x.tar.gz",
            "a/b.tar.gz",
            "a\\b.tar.gz",
            "..tar.gz",
            ".tar.gz",
            "x.tgz",
            "x.tar.gz/..",
        ] {
            assert!(!is_backup_name(bad), "{bad}");
        }
        assert!(is_safe_member(".config/hypr/bindings.lua"));
        assert!(is_safe_member("./.config/hypr"));
        for bad in ["/etc/passwd", "../x", ".config/../../x", ""] {
            assert!(!is_safe_member(bad), "{bad}");
        }
        assert_eq!(
            backup_label("before-syncing-1712345678-2.tar.gz"),
            "Before syncing"
        );
        assert_eq!(
            backup_label("before-other-omarchy-settings.tar.gz"),
            "Before other Omarchy settings"
        );
    }

    #[test]
    fn a_backup_with_a_path_outside_home_is_neither_listed_nor_restored() {
        let home = TempDir::new("restore").unwrap();
        let ctx = Ctx {
            home: home.path().to_path_buf(),
            src: home.path().join("src"),
            host: "h".into(),
            hostname: "h".into(),
        };
        let outside = TempDir::new("outside").unwrap();
        let target = outside.path().join("victim");
        std::fs::write(&target, "safe").unwrap();
        std::fs::create_dir_all(ctx.backups()).unwrap();
        let evil = ctx.backups().join("evil.tar.gz");
        let (e, t) = (evil.to_string_lossy(), target.to_string_lossy());
        // -P keeps the absolute path in the archive.
        run(None, "tar", &["-czPf", &e, &t]).unwrap();
        std::fs::write(&target, "changed since").unwrap();
        assert!(list_backups(&ctx).is_empty());
        assert!(restore_backup(&ctx, "evil.tar.gz").is_err());
        assert!(restore_backup(&ctx, "../evil.tar.gz").is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "changed since");
    }
}
