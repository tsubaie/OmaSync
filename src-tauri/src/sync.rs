//! Everything that touches chezmoi, git, gh and pacman. No UI code here.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const REPO: &str = "omarchy-sync";

/// Folders offered on the "What's synced" tab: (path under home, plain name, on by default).
pub const FOLDERS: &[(&str, &str, bool)] = &[
    (
        ".config/omarchy",
        "Themes, backgrounds, menus and hooks",
        true,
    ),
    // Inside .config/omarchy, but synced on their own (see NESTED).
    (".config/omarchy/plugins", "Plugins", true),
    (
        ".config/omarchy/shell.json",
        "Bar layout and plugin settings",
        true,
    ),
    (
        ".config/hypr",
        "Windows, keyboard shortcuts and input",
        true,
    ),
    (
        ".local/share/applications",
        "Web apps and app shortcuts",
        true,
    ),
    (".config/alacritty", "Alacritty terminal", true),
    (".config/ghostty", "Ghostty terminal", true),
    (".config/kitty", "Kitty terminal", true),
    (".config/foot", "Foot terminal", true),
    (".config/starship.toml", "Terminal prompt", true),
    (".config/nvim", "Neovim editor", false),
    (".config/btop", "System monitor", false),
    (".config/lazygit", "Lazygit", false),
    (".config/fastfetch", "System info screen", false),
    (".config/git", "Git settings", false),
];

/// The data file label shown when only the app list or sync choices changed.
pub const DATA_LABEL: &str = "Apps and sync choices";

/// Folders inside another synced folder; the ignore rules below leave them out when they're off.
pub const NESTED: &[&str] = &[".config/omarchy/plugins", ".config/omarchy/shell.json"];

// monitors.lua differs per screen, so it never leaves the computer it was made on.
const IGNORE: &str = r#"**/.git
**/.git/**
**/node_modules/**
**/*.bak.*
.config/hypr/monitors.lua
.config/hypr/monitors.conf
{{- range list ".config/omarchy/plugins" ".config/omarchy/shell.json" }}
{{- if not (has . $.folders) }}
{{ . }}
{{ . }}/**
{{- end }}
{{- end }}
{{- $c := get .computers (include "/etc/machine-id" | trim | trunc 12) | default dict }}
{{- range get $c "skip_files" | default list }}
{{ . }}
{{- end }}
"#;

/// Base system packages that are never offered as "apps".
const SYSTEM: &[&str] = &[
    "base",
    "linux",
    "linux-firmware",
    "sudo",
    "mkinitcpio",
    "efibootmgr",
    "intel-ucode",
    "amd-ucode",
];

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Data {
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default)]
    pub computers: BTreeMap<String, Computer>,
    /// The layout of this file; older layouts are upgraded when read.
    #[serde(default)]
    pub version: u32,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Computer {
    /// What people call this computer; it names itself, starting from its hostname.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Names this computer synced under before (hostnames from older versions), so its
    /// earlier history still resolves to it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aka: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_apps: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_files: Vec<String>,
    /// Turns off the background timer for this computer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub manual: bool,
    /// Installs synced apps during a sync instead of waiting for approval.
    #[serde(default, skip_serializing_if = "is_false")]
    pub install_without_asking: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

impl Data {
    /// Older versions keyed computers by hostname, and Omarchy's default hostname is "omarchy"
    /// on every machine. Moves this computer's hostname entry to its machine ID, once.
    pub fn adopt_legacy(&mut self, id: &str, hostname: &str) {
        if hostname.is_empty() || hostname == id || self.computers.contains_key(id) {
            return;
        }
        if let Some(mut old) = self.computers.remove(hostname) {
            old.aka.push(hostname.to_string());
            if old.name.is_empty() {
                old.name = hostname.to_string();
            }
            self.computers.insert(id.to_string(), old);
        }
    }

    /// Version 1 split plugins and the bar's settings out of `.config/omarchy`; anyone who
    /// synced that folder keeps syncing both.
    pub fn upgrade(&mut self) {
        if self.version < 1 && self.folders.iter().any(|f| f == ".config/omarchy") {
            for f in NESTED {
                toggle(&mut self.folders, f, true);
            }
        }
        self.version = 1;
    }

    /// The computer ID behind a git author name: an ID, or an older name listed in `aka`.
    pub fn resolve(&self, author: &str) -> String {
        if self.computers.contains_key(author) {
            return author.to_string();
        }
        self.computers
            .iter()
            .find(|(_, c)| c.aka.iter().any(|a| a == author))
            .map_or(author.to_string(), |(id, _)| id.clone())
    }

    /// A name for a new computer: its hostname, made unique if another computer already uses it.
    pub fn fresh_name(&self, id: &str, hostname: &str) -> String {
        let taken = |n: &str| {
            self.computers
                .iter()
                .any(|(other, c)| other != id && c.name == n)
        };
        let base = if hostname.is_empty() {
            "Computer"
        } else {
            hostname
        };
        (1..)
            .map(|i| {
                if i == 1 {
                    base.to_string()
                } else {
                    format!("{base} {i}")
                }
            })
            .find(|n| !taken(n))
            .unwrap()
    }

    pub fn wants_app(&self, host: &str, app: &str) -> bool {
        self.apps.iter().any(|a| a == app)
            && !self
                .computers
                .get(host)
                .is_some_and(|c| c.skip_apps.iter().any(|a| a == app))
    }

    pub fn uses_folder(&self, host: &str, folder: &str) -> bool {
        !self
            .computers
            .get(host)
            .is_some_and(|c| c.skip_files.iter().any(|f| f == folder))
    }

    pub fn set_app(&mut self, host: &str, app: &str, on: bool) {
        toggle(
            &mut self.computers.entry(host.into()).or_default().skip_apps,
            app,
            !on,
        );
    }

    pub fn set_folder_here(&mut self, host: &str, folder: &str, on: bool) {
        toggle(
            &mut self.computers.entry(host.into()).or_default().skip_files,
            folder,
            !on,
        );
    }

    /// Folders this computer should send and receive.
    pub fn active_folders(&self, host: &str) -> Vec<String> {
        self.folders
            .iter()
            .filter(|f| self.uses_folder(host, f))
            .cloned()
            .collect()
    }
}

/// Adds or removes `item` so that `list.contains(item) == present`.
pub fn toggle(list: &mut Vec<String>, item: &str, present: bool) {
    list.retain(|x| x != item);
    if present {
        list.push(item.into());
    }
}

#[derive(Clone)]
pub struct Ctx {
    pub home: PathBuf,
    pub src: PathBuf,
    /// This computer's ID: the first 12 characters of /etc/machine-id, which every systemd Linux
    /// has and which differs per installation even when hostnames match. The ignore template
    /// reads the same file.
    pub host: String,
    pub hostname: String,
}

impl Ctx {
    pub fn load() -> Ctx {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let data_home = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".local/share"));
        let hostname = run(None, "uname", &["-n"]).unwrap_or_default();
        let hostname = hostname.split('.').next().unwrap_or("").to_string();
        let id = std::fs::read_to_string("/etc/machine-id").unwrap_or_default();
        let id: String = id.trim().chars().take(12).collect();
        Ctx {
            // shortcut: assumes chezmoi's default source folder, upgrade to `chezmoi source-path` if anyone sets sourceDir.
            src: data_home.join("chezmoi"),
            host: if id.is_empty() { hostname.clone() } else { id },
            hostname,
            home,
        }
    }

    fn data_path(&self) -> PathBuf {
        self.src.join(".chezmoidata.yaml")
    }

    pub fn load_data(&self) -> Data {
        let mut data: Data = std::fs::read_to_string(self.data_path())
            .ok()
            .and_then(|s| serde_yaml::from_str(&s).ok())
            .unwrap_or_default();
        data.adopt_legacy(&self.host, &self.hostname);
        data.upgrade();
        data
    }

    pub fn save_data(&self, data: &Data) -> Result<(), String> {
        let yaml = serde_yaml::to_string(data).map_err(|e| e.to_string())?;
        std::fs::write(self.data_path(), yaml).map_err(|e| e.to_string())
    }

    fn git(&self, args: &[&str]) -> Result<String, String> {
        run(Some(&self.src), "git", args)
    }

    fn theme_file(&self) -> PathBuf {
        let state = std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or(self.home.join(".local/state"));
        state.join("omarchy/current/theme.name")
    }

    pub fn current_theme(&self) -> Option<String> {
        std::fs::read_to_string(self.theme_file())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

pub fn run(dir: Option<&Path>, prog: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(prog);
    cmd.args(args).stdin(Stdio::null()).env("LC_ALL", "C");
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let out = cmd.output().map_err(|e| format!("{prog}: {e}"))?;
    if out.status.success() {
        // Only the end: git status starts lines with a meaningful space.
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("{prog} {}: {}", args.join(" "), err.trim()))
    }
}

/// Runs the tool rather than looking it up, so a broken shim (e.g. mise with no version set) counts as missing.
pub fn has(prog: &str) -> bool {
    run(None, prog, &["--version"]).is_ok()
}

/// Turns a raw command error into a sentence a person can act on.
pub fn friendly(err: &str) -> String {
    let e = err.to_lowercase();
    if e.contains("could not resolve")
        || e.contains("unable to access")
        || e.contains("network is unreachable")
    {
        "Couldn't reach GitHub. Check your internet connection, then try again.".into()
    } else if e.contains("pkexec") && (e.contains("dismissed") || e.contains("not authorized")) {
        "The password window was closed, so nothing was installed.".into()
    } else if e.contains("another sync") {
        "Another sync is already running. Try again in a moment.".into()
    } else if e.contains("can't undo") {
        "This change can't be undone on its own, because later changes build on it.".into()
    } else if e.contains("rejected") || e.contains("non-fast-forward") {
        "Another computer synced at the same moment. Press Sync now again.".into()
    } else {
        "Something went wrong while syncing. Nothing on this computer was lost.".into()
    }
}

// ---------- First-time setup ----------

#[derive(Clone, Debug, Serialize)]
pub struct SetupState {
    /// Arch packages that must be installed before anything else.
    pub missing: Vec<&'static str>,
    pub signed_in: bool,
    /// Some(login) once signed in; `found` says whether that account already has synced settings.
    pub login: Option<String>,
    pub found: bool,
    pub ready: bool,
}

pub fn probe(ctx: &Ctx) -> SetupState {
    let missing: Vec<&'static str> = [("chezmoi", "chezmoi"), ("gh", "github-cli"), ("git", "git")]
        .into_iter()
        .filter(|(bin, _)| !has(bin))
        .map(|(_, pkg)| pkg)
        .collect();
    let mut s = SetupState {
        ready: false,
        signed_in: false,
        login: None,
        found: false,
        missing,
    };
    if !s.missing.is_empty() {
        return s;
    }
    if is_set_up(ctx) {
        s.ready = true;
        return s;
    }
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

pub fn install_helpers(pkgs: &[&str]) -> Result<(), String> {
    let mut args = vec!["pacman", "-S", "--needed", "--noconfirm"];
    args.extend_from_slice(pkgs);
    run(None, "pkexec", &args).map(|_| ())
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

/// First computer: save its settings into a new private GitHub repo.
pub fn start(ctx: &Ctx, step: &dyn Fn(&str)) -> Result<(), String> {
    step("Getting ready");
    run(None, "gh", &["auth", "setup-git"])?;
    step("Saving this computer's settings");
    seed(ctx)?;
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

/// Creates the local sync folder from this computer's settings and commits it.
fn seed(ctx: &Ctx) -> Result<(), String> {
    run(None, "chezmoi", &["init"])?;
    std::fs::write(ctx.src.join(".chezmoiignore"), IGNORE).map_err(|e| e.to_string())?;
    let mut data = Data {
        folders: FOLDERS
            .iter()
            .filter(|(p, _, on)| *on && ctx.home.join(p).exists())
            .map(|(p, _, _)| p.to_string())
            .collect(),
        theme: ctx.current_theme(),
        ..Default::default()
    };
    data.computers.entry(ctx.host.clone()).or_default().name = ctx.hostname.clone();
    ctx.save_data(&data)?;
    stage(ctx, &data)?;
    commit(ctx)
}

/// Another computer already syncs: back up this one, then take the synced settings.
pub fn join(ctx: &Ctx, login: &str, step: &dyn Fn(&str)) -> Result<PathBuf, String> {
    step("Getting ready");
    run(None, "gh", &["auth", "setup-git"])?;
    adopt(ctx, &format!("https://github.com/{login}/{REPO}.git"), step)
}

fn adopt(ctx: &Ctx, url: &str, step: &dyn Fn(&str)) -> Result<PathBuf, String> {
    step("Downloading your synced settings");
    run(None, "chezmoi", &["init", url])?;
    let data = ctx.load_data();
    step("Backing up this computer's current settings");
    let backup = backup(ctx, &data.folders)?;
    step("Updating this computer");
    run(None, "chezmoi", &["apply", "--force"])?;
    Ok(backup)
}

fn backup(ctx: &Ctx, folders: &[String]) -> Result<PathBuf, String> {
    let dir = ctx.home.join(".local/share/omarchy-sync/backups");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("before-first-sync-{}.tar.gz", now()));
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

// ---------- Syncing ----------

/// Copies this computer's current files into the sync folder (not yet shared).
fn stage(ctx: &Ctx, data: &Data) -> Result<(), String> {
    let paths: Vec<String> = data
        .active_folders(&ctx.host)
        .iter()
        .map(|f| ctx.home.join(f))
        .filter(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec!["add"];
    args.extend(paths.iter().map(|s| s.as_str()));
    run(None, "chezmoi", &args).map(|_| ())
}

fn commit(ctx: &Ctx) -> Result<(), String> {
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
    ctx.git(&["commit", "-q", "-m", &format!("Sync from {}", ctx.host)])
        .map(|_| ())
}

#[derive(Clone, Copy, PartialEq, Debug, Deserialize)]
pub enum Prefer {
    Ask,
    ThisComputer,
    OtherComputer,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "detail")]
pub enum SyncError {
    Conflict(Conflict),
    Failed(String),
}

/// The same lines changed on two computers.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    /// Affected paths under home.
    pub files: Vec<String>,
    /// The other computer that changed them, and when (unix time).
    pub by: Option<(String, i64)>,
}

/// What a finished sync has to say: notes for the person, and apps waiting for their approval.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub notes: Vec<String>,
    pub waiting: Vec<String>,
}

/// Only one sync at a time, whether it comes from the window or the background timer.
fn lock(ctx: &Ctx) -> Result<std::fs::File, String> {
    let file =
        std::fs::File::create(ctx.src.join(".git/omarchy-sync.lock")).map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|_| "another sync is running".to_string())?;
    Ok(file)
}

/// Waits for any running sync (window or background timer) to finish, then holds the lock.
/// Used by the read-mostly overview, which also writes to the sync folder while staging.
fn lock_wait(ctx: &Ctx) -> Result<std::fs::File, String> {
    let file =
        std::fs::File::create(ctx.src.join(".git/omarchy-sync.lock")).map_err(|e| e.to_string())?;
    file.lock().map_err(|e| e.to_string())?;
    Ok(file)
}

/// Commits this computer's current files (and theme) so nothing local is ever overwritten unsaved.
fn save_local(ctx: &Ctx) -> Result<(), String> {
    let mut data = ctx.load_data();
    if data
        .computers
        .get(&ctx.host)
        .is_none_or(|c| c.name.is_empty())
    {
        let name = data.fresh_name(&ctx.host, &ctx.hostname);
        data.computers.entry(ctx.host.clone()).or_default().name = name;
    }
    // Repos made by older versions get the current ignore rules (keyed by machine ID).
    let ignore = ctx.src.join(".chezmoiignore");
    if std::fs::read_to_string(&ignore).is_ok_and(|current| current != IGNORE) {
        std::fs::write(&ignore, IGNORE).map_err(|e| e.to_string())?;
    }
    if let Some(t) = ctx.current_theme() {
        data.theme = Some(t);
    }
    ctx.save_data(&data)?;
    stage(ctx, &data)?;
    commit(ctx)
}

pub fn sync(
    ctx: &Ctx,
    prefer: Prefer,
    install: bool,
    step: &dyn Fn(&str),
) -> Result<Report, SyncError> {
    let fail = SyncError::Failed;
    let _lock = lock(ctx).map_err(fail)?;
    let mut notes = Vec::new();

    step("Saving this computer's changes");
    save_local(ctx).map_err(fail)?;

    step("Getting changes from your other computers");
    let tracked = ctx.git(&["rev-parse", "--abbrev-ref", "@{u}"]).is_ok();
    if tracked {
        let mut args = vec!["pull", "--rebase", "-q"];
        // During a rebase, "theirs" is this computer's commits and "ours" is what came from GitHub.
        match prefer {
            Prefer::ThisComputer => args.extend(["-X", "theirs"]),
            Prefer::OtherComputer => args.extend(["-X", "ours"]),
            Prefer::Ask => {}
        }
        if let Err(e) = ctx.git(&args) {
            let git_dir = ctx.src.join(".git");
            if git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists() {
                let files: Vec<String> = ctx
                    .git(&["diff", "--name-only", "--diff-filter=U"])
                    .unwrap_or_default()
                    .lines()
                    .map(String::from)
                    .collect();
                let _ = ctx.git(&["rebase", "--abort"]);
                // The newest change to these files on GitHub names the other computer.
                let mut args = vec!["log", "-1", "--format=%an%x09%ct", "@{u}", "--"];
                args.extend(files.iter().map(|f| f.as_str()));
                let data = ctx.load_data();
                let by = ctx
                    .git(&args)
                    .ok()
                    .and_then(|l| parse_log(&l).into_iter().next())
                    .map(|(author, t)| (data.resolve(&author), t));
                return Err(SyncError::Conflict(Conflict {
                    files: to_targets(ctx, files),
                    by,
                }));
            }
            return Err(fail(e));
        }
    }

    step("Updating this computer");
    run(None, "chezmoi", &["apply", "--force"]).map_err(fail)?;
    let data = ctx.load_data();
    if let Some(theme) = &data.theme
        && ctx.current_theme().as_ref() != Some(theme)
        && run(None, "omarchy", &["theme", "set", theme]).is_err()
    {
        notes.push(format!("The {theme} theme couldn't be switched on here."));
    }

    step("Sharing with your other computers");
    if tracked {
        ctx.git(&["push", "-q"]).map_err(fail)?;
    } else {
        ctx.git(&["push", "-q", "-u", "origin", "HEAD"])
            .map_err(fail)?;
    }

    let missing = missing_apps(&data, &ctx.host, &installed());
    if missing.is_empty() || !install {
        return Ok(Report {
            notes,
            waiting: missing,
        });
    }
    step(&format!("Installing {}", plural(missing.len(), "app")));
    match install_apps(&missing) {
        Ok(()) => notes.push(format!("Installed {}.", missing.join(", "))),
        Err(e) => notes.push(format!(
            "Couldn't install {}. {}",
            missing.join(", "),
            friendly(&e)
        )),
    }
    Ok(Report {
        notes,
        waiting: vec![],
    })
}

/// Synced apps this computer should have but doesn't.
pub fn missing_apps(data: &Data, host: &str, installed: &BTreeSet<String>) -> Vec<String> {
    data.apps
        .iter()
        .filter(|a| data.wants_app(host, a) && !installed.contains(*a))
        .cloned()
        .collect()
}

pub fn install_apps(apps: &[String]) -> Result<(), String> {
    let mut args = vec!["-S", "--needed", "--noconfirm", "--sudo", "pkexec", "--"];
    args.extend(apps.iter().map(|s| s.as_str()));
    run(None, "yay", &args).map(|_| ())
}

// ---------- History and undo ----------

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub sha: String,
    pub computer: String,
    pub when: i64,
    /// Plain names of what changed, e.g. "Windows, keyboard shortcuts and input".
    pub labels: Vec<String>,
    pub is_undo: bool,
    /// The very first save, which has nothing before it to go back to.
    pub first: bool,
}

const UNDO_PREFIX: &str = "Undo: ";

pub fn history(ctx: &Ctx) -> Vec<Entry> {
    let log = ctx
        .git(&[
            "log",
            "-n",
            "50",
            "--name-only",
            "--format=%x1e%H%x09%an%x09%ct%x09%s%x09%P",
        ])
        .unwrap_or_default();
    parse_history(&log)
}

/// Parses `git log --name-only --format=%x1e%H%x09%an%x09%ct%x09%s%x09%P`.
pub fn parse_history(log: &str) -> Vec<Entry> {
    log.split('\x1e')
        .filter_map(|block| {
            let mut lines = block.lines();
            let head: Vec<&str> = lines.next()?.split('\t').collect();
            let [sha, computer, when, subject, parents] = head[..] else {
                return None;
            };
            let mut labels: Vec<String> = Vec::new();
            for file in lines.filter(|l| !l.is_empty() && !l.starts_with(".chezmoiignore")) {
                let name = label(&source_to_target(file)).to_string();
                if !labels.contains(&name) {
                    labels.push(name);
                }
            }
            Some(Entry {
                sha: sha.into(),
                computer: computer.into(),
                when: when.parse().ok()?,
                labels,
                is_undo: subject.starts_with(UNDO_PREFIX),
                first: parents.is_empty(),
            })
        })
        .collect()
}

/// chezmoi's source name for a file back to its path under home (dot_config -> .config).
pub fn source_to_target(path: &str) -> String {
    if path == ".chezmoidata.yaml" {
        return DATA_LABEL.into();
    }
    const ATTRS: &[&str] = &[
        "private_",
        "readonly_",
        "empty_",
        "executable_",
        "exact_",
        "symlink_",
        "create_",
        "modify_",
        "encrypted_",
    ];
    path.split('/')
        .map(|part| {
            let mut p = part;
            while let Some(rest) = ATTRS.iter().find_map(|a| p.strip_prefix(a)) {
                p = rest;
            }
            p.strip_prefix("dot_")
                .map_or(p.to_string(), |r| format!(".{r}"))
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Reverses one earlier change. The caller then syncs, which applies it here and shares it.
pub fn undo(ctx: &Ctx, sha: &str) -> Result<(), String> {
    let _lock = lock(ctx)?;
    save_local(ctx)?;
    if ctx.git(&["revert", "--no-commit", sha]).is_err() {
        let _ = ctx.git(&["revert", "--abort"]);
        return Err("can't undo: later changes depend on it".into());
    }
    let who = ctx
        .git(&["log", "-1", "--format=%an", sha])
        .unwrap_or_default();
    ctx.git(&[
        "commit",
        "-q",
        "-m",
        &format!("{UNDO_PREFIX}a change from {who}"),
    ])?;
    // Put the undone files in place now; otherwise the next sync would re-save the old ones over the undo.
    run(None, "chezmoi", &["apply", "--force"]).map(|_| ())
}

// ---------- Background syncing ----------

const UNIT: &str = "omarchy-sync";

fn unit_dir(ctx: &Ctx) -> PathBuf {
    std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(ctx.home.join(".config"))
        .join("systemd/user")
}

/// Turns the every-15-minutes background sync on or off for this computer.
pub fn schedule(ctx: &Ctx, on: bool) -> Result<(), String> {
    let enabled = run(
        None,
        "systemctl",
        &["--user", "is-enabled", &format!("{UNIT}.timer")],
    )
    .is_ok_and(|s| s == "enabled");
    if !on {
        return if enabled {
            run(
                None,
                "systemctl",
                &["--user", "disable", "--now", &format!("{UNIT}.timer")],
            )
            .map(|_| ())
        } else {
            Ok(())
        };
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let service = format!(
        "[Unit]\nDescription=Omarchy Sync\n\n[Service]\nType=oneshot\nExecStart={} --background\n",
        exe.display()
    );
    let timer = "[Unit]\nDescription=Sync Omarchy settings every 15 minutes\n\n[Timer]\nOnBootSec=3min\nOnUnitActiveSec=15min\n\n[Install]\nWantedBy=timers.target\n";
    let dir = unit_dir(ctx);
    let service_path = dir.join(format!("{UNIT}.service"));
    if enabled && std::fs::read_to_string(&service_path).is_ok_and(|s| s == service) {
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&service_path, service).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{UNIT}.timer")), timer).map_err(|e| e.to_string())?;
    run(None, "systemctl", &["--user", "daemon-reload"])?;
    run(
        None,
        "systemctl",
        &["--user", "enable", "--now", &format!("{UNIT}.timer")],
    )
    .map(|_| ())
}

pub fn notify(title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .args([
            "-a",
            "Omarchy Sync",
            "-i",
            "emblem-synchronizing",
            title,
            body,
        ])
        .status();
}

/// A tiny key=value file remembering what the background sync already told the person.
fn state_file(ctx: &Ctx) -> PathBuf {
    let state = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or(ctx.home.join(".local/state"));
    state.join("omarchy-sync/background")
}

fn read_state(ctx: &Ctx) -> BTreeMap<String, String> {
    std::fs::read_to_string(state_file(ctx))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn write_state(ctx: &Ctx, state: &BTreeMap<String, String>) {
    let path = state_file(ctx);
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::write(
        path,
        state
            .iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect::<String>(),
    );
}

/// Run by the systemd timer: sync quietly, and notify only when a person is needed.
pub fn background(ctx: &Ctx) {
    if !is_set_up(ctx) {
        return;
    }
    let me = ctx
        .load_data()
        .computers
        .get(&ctx.host)
        .cloned()
        .unwrap_or_default();
    if me.manual {
        return;
    }
    let mut state = read_state(ctx);
    match sync(ctx, Prefer::Ask, me.install_without_asking, &|_| {}) {
        Ok(report) => {
            state.remove("failures");
            state.remove("conflict");
            let waiting = report.waiting.join(", ");
            if !waiting.is_empty() && state.get("waiting") != Some(&waiting) {
                notify(
                    "Apps are ready to install",
                    &format!("{waiting}. Open Omarchy Sync to install them."),
                );
            }
            state.insert("waiting".into(), waiting);
        }
        Err(SyncError::Conflict(_)) => {
            if state.insert("conflict".into(), "1".into()).is_none() {
                notify(
                    "Omarchy Sync needs you",
                    "The same setting was changed on two computers. Open Omarchy Sync to choose which version to keep.",
                );
            }
        }
        Err(SyncError::Failed(e)) if e.contains("another sync") => {}
        Err(SyncError::Failed(e)) => {
            let failures = state
                .get("failures")
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0)
                + 1;
            // One failure is usually a blip (offline, asleep); twice in a row is worth a word.
            if failures == 2 {
                notify("Syncing isn't working", &friendly(&e));
            }
            state.insert("failures".into(), failures.to_string());
        }
    }
    write_state(ctx, &state);
}

// ---------- What the main screen shows ----------

#[derive(Clone, Debug, Default, Serialize)]
pub struct Overview {
    /// Paths under home (or DATA_LABEL) changed here and not shared yet.
    pub unshared: Vec<String>,
    pub incoming: usize,
    pub online: bool,
    /// (computer name, last sync as unix time), this computer first.
    pub computers: Vec<(String, Option<i64>)>,
    pub apps: Apps,
    /// What the other computers' waiting updates would change here, and who made them.
    pub incoming_files: Vec<String>,
    pub incoming_from: Vec<String>,
    pub history: Vec<Entry>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Apps {
    pub installed: BTreeSet<String>,
    /// Installed here by the person (not part of Omarchy) and not synced yet.
    pub local_only: Vec<String>,
    pub about: BTreeMap<String, String>,
}

pub fn overview(ctx: &Ctx, step: &dyn Fn(&str)) -> Result<Overview, String> {
    // Staging and git status would collide with a background sync mid-commit.
    let _lock = lock_wait(ctx)?;
    let data = ctx.load_data();
    step("Looking for changes on this computer");
    stage(ctx, &data)?;
    step("Checking GitHub for your other computers");
    let online = ctx.git(&["fetch", "-q"]).is_ok();
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
    step("Checking your apps");
    let apps = apps(&data);
    Ok(Overview {
        unshared: to_targets(ctx, parse_status(&status)),
        incoming,
        online,
        computers: computers(&parse_log(&log), &data, &ctx.host),
        apps,
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
    })
}

pub fn installed() -> BTreeSet<String> {
    run(None, "pacman", &["-Qq"])
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

fn apps(data: &Data) -> Apps {
    let mut base: BTreeSet<String> = BTreeSet::new();
    for f in ["omarchy-base.packages", "omarchy-other.packages"] {
        let text =
            std::fs::read_to_string(format!("/usr/share/omarchy/install/{f}")).unwrap_or_default();
        base.extend(
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(String::from),
        );
    }
    let explicit = run(None, "pacman", &["-Qqe"]).unwrap_or_default();
    let local_only = user_apps(explicit.lines(), &base)
        .into_iter()
        .filter(|a| !data.apps.contains(a))
        .collect::<Vec<_>>();
    let mut names: Vec<&str> = local_only.iter().map(|s| s.as_str()).collect();
    let installed = installed();
    names.extend(
        data.apps
            .iter()
            .filter(|a| installed.contains(*a))
            .map(|s| s.as_str()),
    );
    let about = if names.is_empty() {
        BTreeMap::new()
    } else {
        let mut args = vec!["-Qi"];
        args.extend(names);
        parse_info(&run(None, "pacman", &args).unwrap_or_default())
    };
    Apps {
        installed,
        local_only,
        about,
    }
}

/// Explicitly installed packages minus Omarchy's own and base system ones.
pub fn user_apps<'a>(
    explicit: impl Iterator<Item = &'a str>,
    base: &BTreeSet<String>,
) -> Vec<String> {
    explicit
        .filter(|p| !base.contains(*p) && !p.starts_with("omarchy") && !SYSTEM.contains(p))
        .map(String::from)
        .collect()
}

/// Maps paths inside the sync folder to paths under home.
fn to_targets(ctx: &Ctx, paths: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut files = Vec::new();
    for p in paths {
        if p == ".chezmoidata.yaml" {
            out.push(DATA_LABEL.to_string());
        } else if !p.starts_with(".chezmoi") {
            files.push(ctx.src.join(p).to_string_lossy().into_owned());
        }
    }
    if !files.is_empty() {
        let mut args = vec!["target-path"];
        args.extend(files.iter().map(|s| s.as_str()));
        let home = format!("{}/", ctx.home.to_string_lossy());
        let targets = run(None, "chezmoi", &args).unwrap_or_default();
        out.extend(
            targets
                .lines()
                .map(|t| t.strip_prefix(&home).unwrap_or(t).to_string()),
        );
    }
    out
}

/// Paths from `git status --porcelain -z`.
pub fn parse_status(s: &str) -> Vec<String> {
    s.split('\0')
        .filter(|e| e.len() > 3)
        .map(|e| e[3..].to_string())
        .collect()
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
fn computers(last: &BTreeMap<String, i64>, data: &Data, host: &str) -> Vec<(String, Option<i64>)> {
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
pub fn parse_info(s: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let mut name = None;
    for line in s.lines() {
        if let Some((k, v)) = line.split_once(" : ") {
            match k.trim() {
                "Name" => name = Some(v.trim().to_string()),
                "Description" => {
                    if let Some(n) = name.take() {
                        out.insert(n, v.trim().to_string());
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// Plain name for a path under home, from FOLDERS.
pub fn label(path: &str) -> &str {
    FOLDERS
        .iter()
        .filter(|(f, _, _)| path == *f || path.starts_with(&format!("{f}/")))
        .max_by_key(|(f, _, _)| f.len())
        .map_or(path, |(_, l, _)| l)
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The web address of this computer's sync space, from the repo's GitHub remote.
pub fn sync_space(ctx: &Ctx) -> Option<String> {
    let remote = ctx.git(&["remote", "get-url", "origin"]).ok()?;
    let path = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("git@github.com:"))?
        .trim_end_matches(".git");
    // Only owner/repo made of safe characters: the page opens this in a browser.
    let safe = path.split('/').count() == 2
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
    safe.then(|| format!("https://github.com/{path}"))
}

/// Stops syncing here: the timer goes off and the local sync folder is moved aside.
/// Settings on this computer stay exactly as they are. Returns where the folder went.
pub fn disconnect(ctx: &Ctx) -> Result<PathBuf, String> {
    let _lock = lock(ctx)?;
    schedule(ctx, false)?;
    let dir = ctx.home.join(".local/share/omarchy-sync");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let moved = dir.join(format!("disconnected-{}", now()));
    std::fs::rename(&ctx.src, &moved).map_err(|e| e.to_string())?;
    Ok(moved)
}

/// Starts or stops syncing one folder from FOLDERS.
pub fn track(ctx: &Ctx, folder: &str, on: bool) -> Result<(), String> {
    let path = ctx.home.join(folder).to_string_lossy().into_owned();
    let args = if on {
        vec!["add", path.as_str()]
    } else {
        vec!["forget", "--force", path.as_str()]
    };
    run(None, "chezmoi", &args).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upgrade_keeps_plugins_syncing_once() {
        let mut d = Data {
            folders: vec![".config/omarchy".into()],
            ..Default::default()
        };
        d.upgrade();
        assert!(NESTED.iter().all(|f| d.folders.iter().any(|x| x == f)));
        // Turned off after the upgrade, they stay off.
        toggle(&mut d.folders, NESTED[0], false);
        d.upgrade();
        assert!(!d.folders.iter().any(|x| x == NESTED[0]));
        let mut fresh = Data::default();
        fresh.upgrade();
        assert!(fresh.folders.is_empty());
    }

    #[test]
    fn apps_respect_per_computer_skips() {
        let mut d = Data {
            apps: vec!["steam".into(), "spotify".into()],
            ..Default::default()
        };
        d.set_app("laptop", "steam", false);
        assert!(!d.wants_app("laptop", "steam"));
        assert!(d.wants_app("laptop", "spotify"));
        assert!(d.wants_app("desktop", "steam"));
        assert!(!d.wants_app("desktop", "zoom"));
        d.set_app("laptop", "steam", true);
        assert!(d.wants_app("laptop", "steam"));
    }

    #[test]
    fn folders_can_be_kept_separate_per_computer() {
        let mut d = Data {
            folders: vec![".config/hypr".into(), ".config/omarchy".into()],
            ..Default::default()
        };
        d.set_folder_here("laptop", ".config/hypr", false);
        assert_eq!(
            d.active_folders("laptop"),
            vec![".config/omarchy".to_string()]
        );
        assert_eq!(d.active_folders("desktop").len(), 2);
    }

    #[test]
    fn data_round_trips_through_yaml_the_ignore_template_reads() {
        let mut d = Data {
            apps: vec!["steam".into()],
            ..Default::default()
        };
        d.set_app("laptop", "steam", false);
        d.computers.entry("desktop".into()).or_default();
        let yaml = serde_yaml::to_string(&d).unwrap();
        assert!(yaml.contains("desktop: {}"), "{yaml}");
        assert_eq!(serde_yaml::from_str::<Data>(&yaml).unwrap(), d);
    }

    #[test]
    fn parsers() {
        assert_eq!(
            parse_status(" M dot_config/hypr/a.lua\0?? .chezmoidata.yaml\0"),
            vec!["dot_config/hypr/a.lua", ".chezmoidata.yaml"]
        );
        let log = parse_log("laptop\t300\ndesktop\t200\nlaptop\t100\n");
        assert_eq!(log["laptop"], 300);
        assert_eq!(log["desktop"], 200);
        let info = parse_info(
            "Name            : brave-bin\nVersion         : 1\nDescription     : Web browser\n\nName            : yazi\nDescription     : File manager\n",
        );
        assert_eq!(info["brave-bin"], "Web browser");
        assert_eq!(info["yazi"], "File manager");
    }

    #[test]
    fn history_reads_git_log_in_plain_words() {
        assert_eq!(
            source_to_target("private_dot_config/omarchy/private_shell.json"),
            ".config/omarchy/shell.json"
        );
        assert_eq!(
            source_to_target("dot_local/share/applications/x.desktop"),
            ".local/share/applications/x.desktop"
        );
        let log = "\x1eaaa\tlaptop\t200\tUndo: a change from desk\tbbb\n\ndot_config/hypr/bindings.lua\n.chezmoidata.yaml\n\x1ebbb\tdesk\t100\tSync from desk\t\n\ndot_config/hypr/input.lua\ndot_config/hypr/bindings.lua\n";
        let h = parse_history(log);
        assert_eq!(h.len(), 2);
        assert_eq!(
            h[0].labels,
            vec![
                "Windows, keyboard shortcuts and input".to_string(),
                DATA_LABEL.to_string()
            ]
        );
        assert!(h[0].is_undo && !h[0].first);
        assert_eq!(
            (
                h[1].computer.as_str(),
                h[1].when,
                h[1].first,
                h[1].labels.len()
            ),
            ("desk", 100, true, 1)
        );
    }

    #[test]
    fn computers_are_identified_by_machine_not_hostname() {
        // Two machines both kept Omarchy's default hostname under the old, hostname-keyed version.
        let mut d = Data::default();
        d.computers.insert(
            "omarchy".into(),
            Computer {
                skip_apps: vec!["steam".into()],
                ..Default::default()
            },
        );
        d.adopt_legacy("aaaa11112222", "omarchy");
        assert!(!d.computers.contains_key("omarchy"));
        let first = &d.computers["aaaa11112222"];
        assert_eq!((first.name.as_str(), first.skip_apps.len()), ("omarchy", 1));
        // Old commits authored "omarchy" now belong to that machine.
        assert_eq!(d.resolve("omarchy"), "aaaa11112222");
        assert_eq!(d.resolve("someone-else"), "someone-else");
        // The second machine gets its own entry and a name that doesn't clash.
        d.adopt_legacy("bbbb33334444", "omarchy");
        assert!(!d.computers.contains_key("bbbb33334444"));
        assert_eq!(d.fresh_name("bbbb33334444", "omarchy"), "omarchy 2");
        assert_eq!(d.fresh_name("aaaa11112222", "omarchy"), "omarchy");
        assert_eq!(d.fresh_name("cccc", ""), "Computer");
    }

    #[test]
    fn missing_apps_only_counts_wanted_and_absent() {
        let mut d = Data {
            apps: vec!["a".into(), "b".into(), "c".into()],
            ..Default::default()
        };
        d.set_app("pc", "c", false);
        let installed: BTreeSet<String> = ["a".to_string()].into();
        assert_eq!(missing_apps(&d, "pc", &installed), vec!["b".to_string()]);
    }

    #[test]
    fn user_apps_skip_omarchy_and_system() {
        let base: BTreeSet<String> = ["hyprland".to_string()].into();
        let got = user_apps(
            ["hyprland", "omarchy-keyring", "sudo", "brave-bin"].into_iter(),
            &base,
        );
        assert_eq!(got, vec!["brave-bin"]);
    }

    #[test]
    fn labels_and_time() {
        assert_eq!(
            label(".config/hypr/bindings.lua"),
            "Windows, keyboard shortcuts and input"
        );
        assert_eq!(label(".config/hyprx/a"), ".config/hyprx/a");
        assert_eq!(plural(1, "app"), "1 app");
        assert_eq!(plural(3, "app"), "3 apps");
    }

    /// Two computers sharing a local bare repo instead of GitHub.
    /// Needs chezmoi and git, so it only runs with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn two_computers_sync_and_settle_a_conflict() {
        let root = std::env::temp_dir().join(format!("omarchy-sync-e2e-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        run(Some(&remote), "git", &["init", "-q", "--bare"]).unwrap();
        let remote = remote.to_string_lossy().into_owned();

        let computer = |name: &str, bindings: &str, monitors: &str| {
            let home = root.join(name);
            std::fs::create_dir_all(home.join(".config/hypr")).unwrap();
            std::fs::write(home.join(".config/hypr/bindings.lua"), bindings).unwrap();
            std::fs::write(home.join(".config/hypr/monitors.lua"), monitors).unwrap();
            Ctx {
                src: home.join(".local/share/chezmoi"),
                host: name.into(),
                hostname: name.into(),
                home,
            }
        };
        // chezmoi and git find their folders through these; never let them reach the real home.
        let enter = |ctx: &Ctx| unsafe {
            std::env::set_var("HOME", &ctx.home);
            std::env::set_var("XDG_CONFIG_HOME", ctx.home.join(".config"));
            std::env::set_var("XDG_DATA_HOME", ctx.home.join(".local/share"));
            std::env::set_var("XDG_STATE_HOME", ctx.home.join(".local/state"));
            std::env::set_var("XDG_CACHE_HOME", ctx.home.join(".cache"));
        };
        let read = |ctx: &Ctx, f: &str| {
            std::fs::read_to_string(ctx.home.join(".config/hypr").join(f)).unwrap()
        };
        let write = |ctx: &Ctx, f: &str, s: &str| {
            std::fs::write(ctx.home.join(".config/hypr").join(f), s).unwrap()
        };
        let quiet = |_: &str| {};
        let ok = |r: Result<Report, SyncError>| match r {
            Ok(_) => {}
            Err(SyncError::Failed(e)) => panic!("{e}"),
            Err(SyncError::Conflict(f)) => panic!("unexpected conflict {f:?}"),
        };

        let a = computer("a", "a1", "screen-a");
        enter(&a);
        seed(&a).unwrap();
        a.git(&["remote", "add", "origin", &remote]).unwrap();
        ok(sync(&a, Prefer::Ask, false, &quiet));

        let b = computer("b", "b-old", "screen-b");
        enter(&b);
        assert!(adopt(&b, &remote, &quiet).unwrap().exists());
        ok(sync(&b, Prefer::Ask, false, &quiet));
        assert_eq!(read(&b, "bindings.lua"), "a1");
        assert_eq!(read(&b, "monitors.lua"), "screen-b");

        // Both change the same file; the second to sync is asked, then keeps its own version.
        enter(&a);
        write(&a, "bindings.lua", "a2");
        ok(sync(&a, Prefer::Ask, false, &quiet));
        enter(&b);
        write(&b, "bindings.lua", "b2");
        let ov = overview(&b, &|_| {}).unwrap();
        assert_eq!(
            (ov.incoming, ov.unshared.clone()),
            (1, vec![".config/hypr/bindings.lua".to_string()])
        );
        match sync(&b, Prefer::Ask, false, &quiet) {
            Err(SyncError::Conflict(c)) => {
                assert_eq!(c.files, vec![".config/hypr/bindings.lua"]);
                assert_eq!(c.by.map(|(name, _)| name), Some("a".to_string()));
            }
            _ => panic!("expected a conflict"),
        }
        assert_eq!(read(&b, "bindings.lua"), "b2");
        ok(sync(&b, Prefer::ThisComputer, false, &quiet));

        enter(&a);
        ok(sync(&a, Prefer::Ask, false, &quiet));
        assert_eq!(read(&a, "bindings.lua"), "b2");
        assert_eq!(read(&a, "monitors.lua"), "screen-a");
        let ov = overview(&a, &|_| {}).unwrap();
        assert_eq!(
            (ov.incoming, ov.unshared.len()),
            (0, 0),
            "{:?}",
            ov.unshared
        );
        assert_eq!(
            ov.computers
                .iter()
                .map(|c| c.0.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );

        // Undo the last change on one computer; the other gets the undo on its next sync.
        let last = &ov.history[0];
        assert!(
            !last.first && last.labels.iter().any(|l| l.starts_with("Windows")),
            "{last:?}"
        );
        write(&a, "bindings.lua", "a3");
        ok(sync(&a, Prefer::Ask, false, &quiet));
        undo(&a, &history(&a)[0].sha).unwrap();
        ok(sync(&a, Prefer::Ask, false, &quiet));
        assert_eq!(read(&a, "bindings.lua"), "b2");
        assert!(history(&a)[0].is_undo);
        enter(&b);
        ok(sync(&b, Prefer::Ask, false, &quiet));
        assert_eq!(read(&b, "bindings.lua"), "b2");
        assert!(history(&b).last().unwrap().first);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
