//! Everything that touches chezmoi, git, gh and pacman. No UI code here.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const REPO: &str = "omarchy-sync";

/// Folders offered on the "What's synced" tab: (path under home, plain name, on by default).
pub const FOLDERS: &[(&str, &str, bool)] = &[
    (".config/omarchy", "Omarchy settings, themes and plugins", true),
    (".config/hypr", "Windows, keyboard shortcuts and input", true),
    (".local/share/applications", "Web apps and app shortcuts", true),
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

// monitors.lua differs per screen, so it never leaves the computer it was made on.
const IGNORE: &str = r#"**/.git
**/.git/**
**/node_modules/**
**/*.bak.*
.config/hypr/monitors.lua
.config/hypr/monitors.conf
{{- $c := get .computers .chezmoi.hostname | default dict }}
{{- range get $c "skip_files" | default list }}
{{ . }}
{{- end }}
"#;

/// Base system packages that are never offered as "apps".
const SYSTEM: &[&str] = &["base", "linux", "linux-firmware", "sudo", "mkinitcpio", "efibootmgr", "intel-ucode", "amd-ucode"];

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
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Computer {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_apps: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_files: Vec<String>,
}

impl Data {
    pub fn wants_app(&self, host: &str, app: &str) -> bool {
        self.apps.iter().any(|a| a == app)
            && !self.computers.get(host).is_some_and(|c| c.skip_apps.iter().any(|a| a == app))
    }

    pub fn uses_folder(&self, host: &str, folder: &str) -> bool {
        !self.computers.get(host).is_some_and(|c| c.skip_files.iter().any(|f| f == folder))
    }

    pub fn set_app(&mut self, host: &str, app: &str, on: bool) {
        toggle(&mut self.computers.entry(host.into()).or_default().skip_apps, app, !on);
    }

    pub fn set_folder_here(&mut self, host: &str, folder: &str, on: bool) {
        toggle(&mut self.computers.entry(host.into()).or_default().skip_files, folder, !on);
    }

    /// Folders this computer should send and receive.
    pub fn active_folders(&self, host: &str) -> Vec<String> {
        self.folders.iter().filter(|f| self.uses_folder(host, f)).cloned().collect()
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
    pub host: String,
}

impl Ctx {
    pub fn load() -> Ctx {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let data_home = std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or(home.join(".local/share"));
        // chezmoi's .chezmoi.hostname is uname -n up to the first dot; the ignore template keys on it.
        let host = run(None, "uname", &["-n"]).unwrap_or_default();
        Ctx {
            // shortcut: assumes chezmoi's default source folder, upgrade to `chezmoi source-path` if anyone sets sourceDir.
            src: data_home.join("chezmoi"),
            host: host.split('.').next().unwrap_or("").to_string(),
            home,
        }
    }

    fn data_path(&self) -> PathBuf {
        self.src.join(".chezmoidata.yaml")
    }

    pub fn load_data(&self) -> Data {
        std::fs::read_to_string(self.data_path())
            .ok()
            .and_then(|s| serde_yaml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_data(&self, data: &Data) -> Result<(), String> {
        let yaml = serde_yaml::to_string(data).map_err(|e| e.to_string())?;
        std::fs::write(self.data_path(), yaml).map_err(|e| e.to_string())
    }

    fn git(&self, args: &[&str]) -> Result<String, String> {
        run(Some(&self.src), "git", args)
    }

    fn theme_file(&self) -> PathBuf {
        let state = std::env::var("XDG_STATE_HOME").map(PathBuf::from).unwrap_or(self.home.join(".local/state"));
        state.join("omarchy/current/theme.name")
    }

    pub fn current_theme(&self) -> Option<String> {
        std::fs::read_to_string(self.theme_file()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
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
    if e.contains("could not resolve") || e.contains("unable to access") || e.contains("network is unreachable") {
        "Couldn't reach GitHub. Check your internet connection, then try again.".into()
    } else if e.contains("pkexec") && (e.contains("dismissed") || e.contains("not authorized")) {
        "The password window was closed, so nothing was installed.".into()
    } else if e.contains("rejected") || e.contains("non-fast-forward") {
        "Another computer synced at the same moment. Press Sync now again.".into()
    } else {
        "Something went wrong while syncing. Nothing on this computer was lost.".into()
    }
}

// ---------- First-time setup ----------

#[derive(Clone, Debug)]
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
    let mut s = SetupState { ready: false, signed_in: false, login: None, found: false, missing };
    if !s.missing.is_empty() {
        return s;
    }
    if ctx.src.join(".git").exists() {
        s.ready = true;
        return s;
    }
    s.signed_in = run(None, "gh", &["auth", "status"]).is_ok();
    if s.signed_in {
        s.login = run(None, "gh", &["api", "user", "--jq", ".login"]).ok();
        if let Some(login) = &s.login {
            s.found = run(None, "gh", &["repo", "view", &format!("{login}/{REPO}"), "--json", "name"]).is_ok();
        }
    }
    s
}

pub fn install_helpers(pkgs: &[&str]) -> Result<(), String> {
    let mut args = vec!["pacman", "-S", "--needed", "--noconfirm"];
    args.extend_from_slice(pkgs);
    run(None, "pkexec", &args).map(|_| ())
}

pub fn sign_in() -> Result<(), String> {
    Command::new("xdg-terminal-exec")
        .args(["gh", "auth", "login", "--web", "--git-protocol", "https", "--hostname", "github.com"])
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
    run(None, "gh", &["repo", "create", REPO, "--private", "--source", &src, "--remote", "origin", "--push"])?;
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
    data.computers.entry(ctx.host.clone()).or_default();
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
    let existing: Vec<&str> = folders.iter().map(|f| f.as_str()).filter(|f| ctx.home.join(f).exists()).collect();
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
    // The author name is the computer's name; "Your computers" reads it back from the log.
    // Set in the repo itself so pulls (which rebase, so they commit) work on computers without a git identity.
    ctx.git(&["config", "user.name", &ctx.host])?;
    ctx.git(&["config", "user.email", &format!("{}@omarchy-sync", ctx.host)])?;
    ctx.git(&["add", "-A"])?;
    if ctx.git(&["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(());
    }
    ctx.git(&["commit", "-q", "-m", &format!("Sync from {}", ctx.host)]).map(|_| ())
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Prefer {
    Ask,
    ThisComputer,
    OtherComputer,
}

#[derive(Debug, Clone)]
pub enum SyncError {
    /// Same lines changed on two computers; holds the affected paths under home.
    Conflict(Vec<String>),
    Failed(String),
}

pub fn sync(ctx: &Ctx, prefer: Prefer, step: &dyn Fn(&str)) -> Result<Vec<String>, SyncError> {
    let fail = SyncError::Failed;
    let mut notes = Vec::new();

    step("Saving this computer's changes");
    let mut data = ctx.load_data();
    data.computers.entry(ctx.host.clone()).or_default();
    if let Some(t) = ctx.current_theme() {
        data.theme = Some(t);
    }
    ctx.save_data(&data).map_err(fail)?;
    stage(ctx, &data).map_err(fail)?;
    commit(ctx).map_err(fail)?;

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
                let files = ctx.git(&["diff", "--name-only", "--diff-filter=U"]).unwrap_or_default();
                let _ = ctx.git(&["rebase", "--abort"]);
                return Err(SyncError::Conflict(to_targets(ctx, files.lines().map(String::from).collect())));
            }
            return Err(fail(e));
        }
    }

    step("Updating this computer");
    run(None, "chezmoi", &["apply", "--force"]).map_err(fail)?;
    let data = ctx.load_data();
    if let Some(theme) = &data.theme {
        if ctx.current_theme().as_ref() != Some(theme) && run(None, "omarchy", &["theme", "set", theme]).is_err() {
            notes.push(format!("The {theme} theme couldn't be switched on here."));
        }
    }

    step("Sharing with your other computers");
    if tracked {
        ctx.git(&["push", "-q"]).map_err(fail)?;
    } else {
        ctx.git(&["push", "-q", "-u", "origin", "HEAD"]).map_err(fail)?;
    }

    let installed = installed();
    let missing: Vec<String> = data.apps.iter().filter(|a| data.wants_app(&ctx.host, a) && !installed.contains(*a)).cloned().collect();
    if !missing.is_empty() {
        step(&format!("Installing {}", plural(missing.len(), "app")));
        let mut args = vec!["-S", "--needed", "--noconfirm", "--sudo", "pkexec", "--"];
        args.extend(missing.iter().map(|s| s.as_str()));
        match run(None, "yay", &args) {
            Ok(_) => notes.push(format!("Installed {}.", missing.join(", "))),
            Err(e) => notes.push(format!("Couldn't install {}. {}", missing.join(", "), friendly(&e))),
        }
    }
    Ok(notes)
}

// ---------- What the main screen shows ----------

#[derive(Clone, Debug, Default)]
pub struct Overview {
    /// Paths under home (or DATA_LABEL) changed here and not shared yet.
    pub unshared: Vec<String>,
    pub incoming: usize,
    pub online: bool,
    /// (computer name, last sync as unix time), this computer first.
    pub computers: Vec<(String, Option<i64>)>,
    pub apps: Apps,
}

#[derive(Clone, Debug, Default)]
pub struct Apps {
    pub installed: BTreeSet<String>,
    /// Installed here by the person (not part of Omarchy) and not synced yet.
    pub local_only: Vec<String>,
    pub about: BTreeMap<String, String>,
}

pub fn overview(ctx: &Ctx) -> Result<Overview, String> {
    let data = ctx.load_data();
    stage(ctx, &data)?;
    let online = ctx.git(&["fetch", "-q"]).is_ok();
    let incoming = ctx.git(&["rev-list", "--count", "HEAD..@{u}"]).ok().and_then(|n| n.parse().ok()).unwrap_or(0);
    let status = ctx.git(&["status", "--porcelain", "-z", "--untracked-files=all"])?;
    let log = ctx.git(&["log", "--format=%an%x09%ct"]).unwrap_or_default();
    Ok(Overview {
        unshared: to_targets(ctx, parse_status(&status)),
        incoming,
        online,
        computers: computers(&parse_log(&log), &data, &ctx.host),
        apps: apps(&data),
    })
}

fn installed() -> BTreeSet<String> {
    run(None, "pacman", &["-Qq"]).unwrap_or_default().lines().map(String::from).collect()
}

fn apps(data: &Data) -> Apps {
    let mut base: BTreeSet<String> = BTreeSet::new();
    for f in ["omarchy-base.packages", "omarchy-other.packages"] {
        let text = std::fs::read_to_string(format!("/usr/share/omarchy/install/{f}")).unwrap_or_default();
        base.extend(text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from));
    }
    let explicit = run(None, "pacman", &["-Qqe"]).unwrap_or_default();
    let local_only = user_apps(explicit.lines(), &base).into_iter().filter(|a| !data.apps.contains(a)).collect::<Vec<_>>();
    let mut names: Vec<&str> = local_only.iter().map(|s| s.as_str()).collect();
    let installed = installed();
    names.extend(data.apps.iter().filter(|a| installed.contains(*a)).map(|s| s.as_str()));
    let about = if names.is_empty() {
        BTreeMap::new()
    } else {
        let mut args = vec!["-Qi"];
        args.extend(names);
        parse_info(&run(None, "pacman", &args).unwrap_or_default())
    };
    Apps { installed, local_only, about }
}

/// Explicitly installed packages minus Omarchy's own and base system ones.
pub fn user_apps<'a>(explicit: impl Iterator<Item = &'a str>, base: &BTreeSet<String>) -> Vec<String> {
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
        out.extend(targets.lines().map(|t| t.strip_prefix(&home).unwrap_or(t).to_string()));
    }
    out
}

/// Paths from `git status --porcelain -z`.
pub fn parse_status(s: &str) -> Vec<String> {
    s.split('\0').filter(|e| e.len() > 3).map(|e| e[3..].to_string()).collect()
}

/// Latest commit time per author from `git log --format=%an%x09%ct` (newest first).
pub fn parse_log(s: &str) -> BTreeMap<String, i64> {
    let mut last = BTreeMap::new();
    for line in s.lines() {
        if let Some((name, t)) = line.split_once('\t') {
            if let Ok(t) = t.parse() {
                last.entry(name.to_string()).or_insert(t);
            }
        }
    }
    last
}

fn computers(last: &BTreeMap<String, i64>, data: &Data, host: &str) -> Vec<(String, Option<i64>)> {
    let mut names: BTreeSet<&String> = data.computers.keys().collect();
    names.extend(last.keys());
    let mut list: Vec<_> = names.into_iter().map(|n| (n.clone(), last.get(n).copied())).collect();
    list.sort_by_key(|(n, _)| n != host);
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
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

pub fn plural(n: usize, word: &str) -> String {
    if n == 1 { format!("1 {word}") } else { format!("{n} {word}s") }
}

pub fn ago(secs: i64) -> String {
    match secs {
        ..60 => "just now".into(),
        ..3600 => format!("{} ago", plural((secs / 60) as usize, "minute")),
        ..86400 => format!("{} ago", plural((secs / 3600) as usize, "hour")),
        _ => format!("{} ago", plural((secs / 86400) as usize, "day")),
    }
}

/// Starts or stops syncing one folder from FOLDERS.
pub fn track(ctx: &Ctx, folder: &str, on: bool) -> Result<(), String> {
    let path = ctx.home.join(folder).to_string_lossy().into_owned();
    let args = if on { vec!["add", path.as_str()] } else { vec!["forget", "--force", path.as_str()] };
    run(None, "chezmoi", &args).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_respect_per_computer_skips() {
        let mut d = Data { apps: vec!["steam".into(), "spotify".into()], ..Default::default() };
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
        let mut d = Data { folders: vec![".config/hypr".into(), ".config/omarchy".into()], ..Default::default() };
        d.set_folder_here("laptop", ".config/hypr", false);
        assert_eq!(d.active_folders("laptop"), vec![".config/omarchy".to_string()]);
        assert_eq!(d.active_folders("desktop").len(), 2);
    }

    #[test]
    fn data_round_trips_through_yaml_the_ignore_template_reads() {
        let mut d = Data { apps: vec!["steam".into()], ..Default::default() };
        d.set_app("laptop", "steam", false);
        d.computers.entry("desktop".into()).or_default();
        let yaml = serde_yaml::to_string(&d).unwrap();
        assert!(yaml.contains("desktop: {}"), "{yaml}");
        assert_eq!(serde_yaml::from_str::<Data>(&yaml).unwrap(), d);
    }

    #[test]
    fn parsers() {
        assert_eq!(parse_status(" M dot_config/hypr/a.lua\0?? .chezmoidata.yaml\0"), vec!["dot_config/hypr/a.lua", ".chezmoidata.yaml"]);
        let log = parse_log("laptop\t300\ndesktop\t200\nlaptop\t100\n");
        assert_eq!(log["laptop"], 300);
        assert_eq!(log["desktop"], 200);
        let info = parse_info("Name            : brave-bin\nVersion         : 1\nDescription     : Web browser\n\nName            : yazi\nDescription     : File manager\n");
        assert_eq!(info["brave-bin"], "Web browser");
        assert_eq!(info["yazi"], "File manager");
    }

    #[test]
    fn user_apps_skip_omarchy_and_system() {
        let base: BTreeSet<String> = ["hyprland".to_string()].into();
        let got = user_apps(["hyprland", "omarchy-keyring", "sudo", "brave-bin"].into_iter(), &base);
        assert_eq!(got, vec!["brave-bin"]);
    }

    #[test]
    fn labels_and_time() {
        assert_eq!(label(".config/hypr/bindings.lua"), "Windows, keyboard shortcuts and input");
        assert_eq!(label(".config/hyprx/a"), ".config/hyprx/a");
        assert_eq!(ago(30), "just now");
        assert_eq!(ago(60), "1 minute ago");
        assert_eq!(ago(7200), "2 hours ago");
        assert_eq!(ago(86400 * 3), "3 days ago");
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
            Ctx { src: home.join(".local/share/chezmoi"), host: name.into(), home }
        };
        // chezmoi and git find their folders through these; never let them reach the real home.
        let enter = |ctx: &Ctx| unsafe {
            std::env::set_var("HOME", &ctx.home);
            std::env::set_var("XDG_CONFIG_HOME", ctx.home.join(".config"));
            std::env::set_var("XDG_DATA_HOME", ctx.home.join(".local/share"));
            std::env::set_var("XDG_STATE_HOME", ctx.home.join(".local/state"));
            std::env::set_var("XDG_CACHE_HOME", ctx.home.join(".cache"));
        };
        let read = |ctx: &Ctx, f: &str| std::fs::read_to_string(ctx.home.join(".config/hypr").join(f)).unwrap();
        let write = |ctx: &Ctx, f: &str, s: &str| std::fs::write(ctx.home.join(".config/hypr").join(f), s).unwrap();
        let quiet = |_: &str| {};
        let ok = |r: Result<Vec<String>, SyncError>| match r {
            Ok(_) => {}
            Err(SyncError::Failed(e)) => panic!("{e}"),
            Err(SyncError::Conflict(f)) => panic!("unexpected conflict {f:?}"),
        };

        let a = computer("a", "a1", "screen-a");
        enter(&a);
        seed(&a).unwrap();
        a.git(&["remote", "add", "origin", &remote]).unwrap();
        ok(sync(&a, Prefer::Ask, &quiet));

        let b = computer("b", "b-old", "screen-b");
        enter(&b);
        assert!(adopt(&b, &remote, &quiet).unwrap().exists());
        ok(sync(&b, Prefer::Ask, &quiet));
        assert_eq!(read(&b, "bindings.lua"), "a1");
        assert_eq!(read(&b, "monitors.lua"), "screen-b");

        // Both change the same file; the second to sync is asked, then keeps its own version.
        enter(&a);
        write(&a, "bindings.lua", "a2");
        ok(sync(&a, Prefer::Ask, &quiet));
        enter(&b);
        write(&b, "bindings.lua", "b2");
        let ov = overview(&b).unwrap();
        assert_eq!((ov.incoming, ov.unshared.clone()), (1, vec![".config/hypr/bindings.lua".to_string()]));
        match sync(&b, Prefer::Ask, &quiet) {
            Err(SyncError::Conflict(files)) => assert_eq!(files, vec![".config/hypr/bindings.lua"]),
            _ => panic!("expected a conflict"),
        }
        assert_eq!(read(&b, "bindings.lua"), "b2");
        ok(sync(&b, Prefer::ThisComputer, &quiet));

        enter(&a);
        ok(sync(&a, Prefer::Ask, &quiet));
        assert_eq!(read(&a, "bindings.lua"), "b2");
        assert_eq!(read(&a, "monitors.lua"), "screen-a");
        let ov = overview(&a).unwrap();
        assert_eq!((ov.incoming, ov.unshared.len()), (0, 0), "{:?}", ov.unshared);
        assert_eq!(ov.computers.iter().map(|c| c.0.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
