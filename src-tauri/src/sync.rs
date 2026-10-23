//! Everything that touches chezmoi, git, gh and pacman. No UI code here.

mod choices;
mod ctx;
mod data;
mod rules;

pub use ctx::{Ctx, edit, friendly, run, sync_space};
// What the updater shares with a sync.
pub(crate) use ctx::{lock, lock_wait, plan};
pub use data::{Data, Script, is_plugin_id, name_taken, toggle};
pub use rules::{DATA_LABEL, FOLDERS};

// Shared by the modules, each of which starts with `use super::*`.
use ctx::*;
use data::*;
use rules::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// Repositories every Omarchy computer has; only others are recorded and offered.
const STANDARD_REPOS: &[&str] = &[
    "omarchy",
    "core",
    "extra",
    "multilib",
    "core-testing",
    "extra-testing",
    "multilib-testing",
];

/// Synced apps this computer wants and `yay` can install (not ones from outside the repos);
/// `here` is `repos_here()`.
pub fn missing_apps(
    data: &Data,
    host: &str,
    installed: &BTreeSet<String>,
    here: &BTreeSet<String>,
) -> Vec<String> {
    data.apps
        .iter()
        .filter(|a| {
            data.wants_app(host, a)
                && !installed.contains(*a)
                && !data.outside.contains_key(*a)
                // From an extra repository this computer doesn't have: never from the AUR
                // instead; it waits for the repository (see missing_repos).
                && data.app_repo.get(*a).is_none_or(|r| here.contains(r))
        })
        .cloned()
        .collect()
}

/// The package repositories this computer uses.
pub fn repos_here() -> BTreeSet<String> {
    run(None, "pacman-conf", &["--repo-list"])
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// An extra repository this computer needs for apps it should have.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MissingRepo {
    pub name: String,
    pub repository: Repository,
    pub apps: Vec<String>,
}

/// Extra repositories this computer lacks for synced apps it wants and doesn't have yet.
pub fn missing_repos(
    data: &Data,
    host: &str,
    installed: &BTreeSet<String>,
    here: &BTreeSet<String>,
) -> Vec<MissingRepo> {
    let mut needed: BTreeMap<&String, Vec<String>> = BTreeMap::new();
    for app in data
        .apps
        .iter()
        .filter(|a| data.wants_app(host, a) && !installed.contains(*a))
    {
        if let Some(repo) = data.app_repo.get(app).filter(|r| !here.contains(*r)) {
            needed.entry(repo).or_default().push(app.clone());
        }
    }
    needed
        .into_iter()
        .filter_map(|(name, apps)| {
            Some(MissingRepo {
                name: name.clone(),
                repository: data.repositories.get(name)?.clone(),
                apps,
            })
        })
        .collect()
}

/// The extra repositories this computer has that are shared, with the person's apps from each.
pub fn repos_with_apps(data: &Data, here: &BTreeSet<String>) -> Vec<MissingRepo> {
    data.repositories
        .iter()
        .filter(|(name, _)| here.contains(*name))
        .map(|(name, repository)| MissingRepo {
            name: name.clone(),
            repository: repository.clone(),
            apps: data
                .app_repo
                .iter()
                .filter(|(_, r)| *r == name)
                .map(|(app, _)| app.clone())
                .collect(),
        })
        .collect()
}

/// This computer's extra repositories as pacman uses them (servers and signature rules), with
/// the key that signs each, and which of the person's apps come from one.
fn record_repositories(data: &mut Data) {
    let extra: Vec<String> = repos_here()
        .into_iter()
        .filter(|r| !STANDARD_REPOS.contains(&r.as_str()))
        .collect();
    if extra.is_empty() {
        return;
    }
    for name in &extra {
        let conf = run(None, "pacman-conf", &[&format!("--repo={name}")]).unwrap_or_default();
        let field = |key: &str| -> Vec<String> {
            conf.lines()
                .filter_map(|l| l.strip_prefix(key))
                .map(|v| v.trim().to_string())
                .collect()
        };
        let repository = Repository {
            servers: field("Server = "),
            sig_level: field("SigLevel = "),
            key: repo_key(name),
        };
        if !repository.servers.is_empty() {
            data.repositories.insert(name.clone(), repository);
        }
    }
    let mine = user_apps_here();
    for (app, repo) in sources(mine.iter(), &[], &BTreeMap::new(), &BTreeMap::new()) {
        if extra.contains(&repo) {
            data.app_repo.insert(app, repo);
        }
    }
}

/// The key that signed a repository's package list, copied from pacman's keyring. The keyring
/// is root's, so its public half is read from a copy in a temporary folder.
fn repo_key(name: &str) -> Option<RepoKey> {
    let db = run(None, "pacman-conf", &["DBPath"]).unwrap_or("/var/lib/pacman/".into());
    let sig = Path::new(db.trim())
        .join("sync")
        .join(format!("{name}.db.sig"));
    let packets = run(None, "gpg", &["--list-packets", &sig.to_string_lossy()]).ok()?;
    let fingerprint = packets
        .split_whitespace()
        .skip_while(|w| *w != "fpr")
        .nth(2)?
        .trim_end_matches(')')
        .to_uppercase();
    if fingerprint.len() != 40 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("omarchy-sync-keys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let ring = ["pubring.gpg", "pubring.kbx"]
        .iter()
        .find(|f| std::fs::copy(Path::new("/etc/pacman.d/gnupg").join(f), dir.join(f)).is_ok());
    let home = dir.to_string_lossy().into_owned();
    let gpg = |args: &[&str]| {
        let mut all = vec!["--homedir", home.as_str(), "--no-autostart", "--batch"];
        all.extend_from_slice(args);
        run(None, "gpg", &all).ok()
    };
    let key = ring.and_then(|_| {
        let armored = gpg(&["--export", "--armor", &fingerprint])?;
        let owner = gpg(&["--list-keys", "--with-colons", &fingerprint])?
            .lines()
            .find_map(|l| {
                l.strip_prefix("uid:")
                    .map(|u| u.split(':').nth(8).unwrap_or("").to_string())
            })
            .unwrap_or_default();
        armored
            .starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----")
            .then(|| RepoKey {
                fingerprint: fingerprint.clone(),
                owner,
                armored,
            })
    });
    let _ = std::fs::remove_dir_all(&dir);
    key
}

/// Adds an extra repository on this computer, as recorded from another one: its settings file
/// in /etc/pacman.d, an Include for it in pacman.conf, its key imported and trusted, and the
/// package lists refreshed. One password prompt (pkexec); every value is checked first.
pub fn add_repository(data: &Data, name: &str) -> Result<(), String> {
    let repo = data
        .repositories
        .get(name)
        .ok_or_else(|| format!("no repository called {name}"))?;
    let plain = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    if !plain(name) || STANDARD_REPOS.contains(&name) {
        return Err(format!("not a repository that can be added: {name}"));
    }
    if repos_here().contains(name) {
        return Ok(());
    }
    const LEVELS: &[&str] = &[
        "Never",
        "Optional",
        "Required",
        "TrustedOnly",
        "TrustAll",
        "PackageNever",
        "PackageOptional",
        "PackageRequired",
        "PackageTrustedOnly",
        "PackageTrustAll",
        "DatabaseNever",
        "DatabaseOptional",
        "DatabaseRequired",
        "DatabaseTrustedOnly",
        "DatabaseTrustAll",
    ];
    let servers_ok = !repo.servers.is_empty()
        && repo.servers.iter().all(|s| {
            s.starts_with("https://") && !s.chars().any(|c| c.is_whitespace() || c.is_control())
        });
    if !servers_ok || !repo.sig_level.iter().all(|l| LEVELS.contains(&l.as_str())) {
        return Err(format!("the settings recorded for {name} don't look right"));
    }
    let dir = std::env::temp_dir().join(format!("omarchy-sync-repo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut conf = format!("[{name}]\n");
    if !repo.sig_level.is_empty() {
        conf.push_str(&format!("SigLevel = {}\n", repo.sig_level.join(" ")));
    }
    for server in &repo.servers {
        conf.push_str(&format!("Server = {server}\n"));
    }
    let target = format!("/etc/pacman.d/omarchy-sync-{name}.conf");
    let include = format!("Include = {target}");
    let mut script = format!(
        "set -e\ninstall -m 644 '{}' '{target}'\ngrep -qxF '{include}' /etc/pacman.conf || printf '\\n%s\\n' '{include}' >> /etc/pacman.conf\n",
        dir.join("repo.conf").display()
    );
    std::fs::write(dir.join("repo.conf"), conf).map_err(|e| e.to_string())?;
    if let Some(key) = &repo.key {
        let fpr_ok =
            key.fingerprint.len() == 40 && key.fingerprint.bytes().all(|b| b.is_ascii_hexdigit());
        if !fpr_ok
            || !key
                .armored
                .starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----")
        {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("the key recorded for {name} doesn't look right"));
        }
        std::fs::write(dir.join("repo.key"), &key.armored).map_err(|e| e.to_string())?;
        script.push_str(&format!(
            "pacman-key --add '{}'\npacman-key --lsign-key {}\n",
            dir.join("repo.key").display(),
            key.fingerprint
        ));
    }
    script.push_str("pacman -Sy --noconfirm\n");
    std::fs::write(dir.join("add.sh"), &script).map_err(|e| e.to_string())?;
    let result = run(
        None,
        "pkexec",
        &["bash", &dir.join("add.sh").to_string_lossy()],
    )
    .map(|_| ());
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Synced apps from outside the repos that this computer wants, with their install scripts.
pub fn missing_scripted(
    data: &Data,
    host: &str,
    installed: &BTreeSet<String>,
) -> Vec<(String, Script)> {
    data.apps
        .iter()
        .filter(|a| data.wants_app(host, a) && !installed.contains(*a))
        .filter_map(|a| Some((a.clone(), data.outside.get(a)?.clone()?)))
        .collect()
}

/// Which of these locally installed packages came from neither Arch's repos nor the AUR.
/// Asks the AUR once per name and remembers it; when it can't be reached, it claims none.
pub fn outside_repos(mine: &[String]) -> Vec<String> {
    use std::sync::{Mutex, OnceLock};
    static IN_AUR: OnceLock<Mutex<BTreeMap<String, bool>>> = OnceLock::new();
    let foreign: BTreeSet<String> = run(None, "pacman", &["-Qqm"])
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    let candidates: Vec<&String> = mine.iter().filter(|a| foreign.contains(*a)).collect();
    let known = IN_AUR.get_or_init(Default::default);
    let unknown: Vec<&String> = {
        let k = known.lock().unwrap_or_else(|e| e.into_inner());
        candidates
            .iter()
            .copied()
            .filter(|a| !k.contains_key(*a))
            .collect()
    };
    if !unknown.is_empty() {
        let query: Vec<String> = unknown
            .iter()
            .map(|a| format!("arg[]={}", a.replace('+', "%2B").replace('@', "%40")))
            .collect();
        let url = format!("https://aur.archlinux.org/rpc/v5/info?{}", query.join("&"));
        let Ok(body) = run(None, "curl", &["-fsS", "--max-time", "8", &url]) else {
            return Vec::new();
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) else {
            return Vec::new();
        };
        let found: BTreeSet<&str> = json["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["Name"].as_str())
            .collect();
        let mut k = known.lock().unwrap_or_else(|e| e.into_inner());
        for a in unknown {
            k.insert(a.clone(), found.contains(a.as_str()));
        }
    }
    let k = known.lock().unwrap_or_else(|e| e.into_inner());
    candidates
        .into_iter()
        .filter(|a| k.get(*a) == Some(&false))
        .cloned()
        .collect()
}

/// Where apps come from. pacman takes a package from the first repository in pacman.conf that
/// has it (on Omarchy: omarchy, then Arch's core, extra and multilib), and yay turns to the AUR
/// only when none does; apps from none of these install with their steps. An app from an extra
/// repository this computer doesn't have (`app_repo`, recorded by another computer) shows that
/// repository rather than the AUR.
pub fn sources<'a>(
    apps: impl Iterator<Item = &'a String>,
    outside_here: &[String],
    outside_synced: &BTreeMap<String, Option<Script>>,
    app_repo: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut repo_of: BTreeMap<&str, &str> = BTreeMap::new();
    let listing = run(None, "pacman", &["-Sl"]).unwrap_or_default();
    // `pacman -Sl` lists repositories in pacman.conf's order; the first one listing a package wins.
    for line in listing.lines() {
        let mut parts = line.split(' ');
        if let (Some(repo), Some(name)) = (parts.next(), parts.next()) {
            repo_of.entry(name).or_insert(repo);
        }
    }
    apps.map(|app| {
        let source = match repo_of.get(app.as_str()) {
            Some(&"omarchy") => "omarchy",
            Some(&("core" | "extra" | "multilib")) => "arch",
            Some(repo) => repo,
            None if outside_here.contains(app) || outside_synced.contains_key(app) => "elsewhere",
            None if app_repo.contains_key(app) => app_repo[app].as_str(),
            None => "aur",
        };
        (app.clone(), source.to_string())
    })
    .collect()
}

/// A pacman package name: lowercase letters, digits and @._+-, not starting with - or a dot.
pub fn is_package_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "@._+-".contains(c))
}

/// Installs an app with its install command, run in a fresh empty folder in a terminal window,
/// so the person sees what it does and can type a password if it asks. Waits for the command
/// to finish, then reports how it ended.
pub fn install_with_script(
    ctx: &Ctx,
    app: &str,
    script: &Script,
    step: &dyn Fn(&str),
) -> Result<(), String> {
    // The name comes from the shared file, which anyone can edit on GitHub: only a plain
    // package name becomes a folder that is emptied below.
    if !is_package_name(app) {
        return Err(format!("not an app: {app}"));
    }
    let dir = ctx.home.join(".cache/omarchy-sync/install").join(app);
    let done = dir.with_extension("done");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&done);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    step(&format!("Installing {app} in a terminal window"));
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    // The steps run as one script that stops at the first failing step, so a failed build
    // never goes on to install.
    let file = dir.with_extension("sh");
    std::fs::write(
        &file,
        format!(
            "set -e -o pipefail\ncd {}\n{}\n",
            quote(&dir.to_string_lossy()),
            script.command
        ),
    )
    .map_err(|e| e.to_string())?;
    let line = format!(
        "bash {}; echo $? > {}; echo; read -rp 'Press Enter to close this window. ' _",
        quote(&file.to_string_lossy()),
        quote(&done.to_string_lossy()),
    );
    Command::new("xdg-terminal-exec")
        .args(["bash", "-c", &line])
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("xdg-terminal-exec: {e}"))?;
    // Some terminals hand the window to an already-running copy and return at once, so the
    // command's own end is the signal: it writes its exit code to `done`.
    for _ in 0..3600 {
        if let Ok(code) = std::fs::read_to_string(&done) {
            let _ = std::fs::remove_file(&done);
            return match code.trim() {
                "0" => Ok(()),
                code => Err(format!(
                    "the install command for {app} stopped with code {code}"
                )),
            };
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    Err(format!(
        "the install command for {app} didn't finish within an hour"
    ))
}

pub fn install_apps(apps: &[String]) -> Result<(), String> {
    let mut args = vec!["-S", "--needed", "--noconfirm", "--sudo", "pkexec", "--"];
    args.extend(apps.iter().map(|s| s.as_str()));
    run(None, "yay", &args).map(|_| ())
}

pub fn installed() -> BTreeSet<String> {
    run(None, "pacman", &["-Qq"])
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// The apps installed here by the person: explicitly installed, not part of Omarchy.
fn user_apps_here() -> Vec<String> {
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
    user_apps(explicit.lines(), &base)
}

pub(crate) fn apps(data: &Data) -> Apps {
    let mine = user_apps_here();
    let installed = installed();
    let mut names: Vec<&str> = mine.iter().map(|s| s.as_str()).collect();
    names.extend(
        data.apps
            .iter()
            .filter(|a| installed.contains(*a) && !mine.contains(a))
            .map(|s| s.as_str()),
    );
    let about = if names.is_empty() {
        BTreeMap::new()
    } else {
        let mut args = vec!["-Qi"];
        args.extend(names);
        parse_info(&run(None, "pacman", &args).unwrap_or_default())
    };
    let outside = outside_repos(&mine);
    // Every app shown: this computer's and the synced ones it may not have yet.
    let all = mine.iter().chain(data.apps.iter());
    let source = sources(all, &outside, &data.outside, &data.app_repo);
    Apps {
        installed,
        mine,
        outside,
        source,
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
pub(crate) fn state_file(ctx: &Ctx) -> PathBuf {
    ctx.state_dir().join("omarchy-sync/background")
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

/// How a background sync went, as far as the person is concerned.
enum Outcome {
    /// Synced; these apps and plugins (by name) wait for a yes here.
    Synced {
        apps: Vec<String>,
        plugins: Vec<String>,
    },
    Clash,
    Failed(String),
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
    let outcome = match sync(ctx, me.install_without_asking, &|_| {}) {
        Ok(report) => {
            // Apps, and plugins other computers have, that wait for a yes here.
            let data = ctx.load_data();
            let mut apps = report.waiting;
            apps.extend(
                missing_scripted(&data, &ctx.host, &installed())
                    .into_iter()
                    .map(|(app, _)| app),
            );
            let plugins = data
                .missing_plugins(&ctx.host)
                .iter()
                .map(|id| ctx.plugin(id).name)
                .collect();
            Outcome::Synced { apps, plugins }
        }
        Err(SyncError::Conflict(_)) => Outcome::Clash,
        Err(SyncError::Failed(e)) => Outcome::Failed(e),
    };
    if let Some((title, body)) = tell(&mut state, &outcome) {
        notify(&title, &body);
    }
    write_state(ctx, &state);
}

/// What to tell the person about an outcome, given what they were already told (`state`,
/// brought up to date here): a clash once, the second failure in a row (one is usually a
/// blip: offline, asleep), and a waiting list whenever it changes.
fn tell(state: &mut BTreeMap<String, String>, outcome: &Outcome) -> Option<(String, String)> {
    match outcome {
        Outcome::Synced { apps, plugins } => {
            state.remove("failures");
            state.remove("conflict");
            let waiting = [apps.as_slice(), plugins.as_slice()].concat().join(", ");
            let changed = !waiting.is_empty() && state.get("waiting") != Some(&waiting);
            state.insert("waiting".into(), waiting.clone());
            changed.then(|| {
                let what = match (apps.is_empty(), plugins.is_empty()) {
                    (false, true) => "Apps are ready to install",
                    (true, false) => "Plugins are ready to add",
                    _ => "Apps and plugins are ready to add",
                };
                (
                    what.to_string(),
                    format!("{waiting}. Open Omarchy Sync to add them."),
                )
            })
        }
        Outcome::Clash => state
            .insert("conflict".into(), "1".into())
            .is_none()
            .then(|| {
                (
                    "Omarchy Sync needs you".to_string(),
                    "The same setting was changed on two computers. Open Omarchy Sync to choose which version to keep.".to_string(),
                )
            }),
        Outcome::Failed(e) => {
            let failures = state
                .get("failures")
                .and_then(|n| n.parse::<u32>().ok())
                .unwrap_or(0)
                + 1;
            state.insert("failures".into(), failures.to_string());
            (failures == 2).then(|| ("Syncing isn't working".to_string(), friendly(e)))
        }
    }
}

/// What the bar indicator shows (packaging/bar-plugin): `state` is synced, syncing, attention
/// or error, and `text` says it in a sentence. Best effort: a failed write only leaves the
/// indicator a step behind.
pub fn write_status(ctx: &Ctx, state: &str, text: &str) {
    let path = state_file(ctx).with_file_name("status.json");
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
    let json = serde_json::json!({ "state": state, "text": text });
    // Written whole and then renamed into place, so the bar never reads half a file.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, json.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// The bar indicator: a small Omarchy shell plugin, kept with the app. The shell keeps a
/// widget's code by file name, so the widget's file name carries a number that goes up
/// whenever it changes (Indicator2, Indicator3: QML only loads names that are valid types,
/// with no hyphens); an older file is replaced, not edited.
pub const BAR_PLUGIN: &str = "io.github.tsubaie.omarchy-sync";

const BAR_FILES: &[(&str, &str)] = &[
    (
        "manifest.json",
        include_str!("../../packaging/bar-plugin/manifest.json"),
    ),
    (
        "Indicator2.qml",
        include_str!("../../packaging/bar-plugin/Indicator2.qml"),
    ),
];

pub fn bar_indicator(ctx: &Ctx) -> bool {
    ctx.home
        .join(PLUGIN_DIR)
        .join(BAR_PLUGIN)
        .join("manifest.json")
        .exists()
}

/// Puts the indicator on the bar unless the person took it off this computer, and brings an
/// installed one up to this version. Cheap when it's already current.
pub fn ensure_bar_indicator(ctx: &Ctx, data: &Data) -> Result<(), String> {
    let off = data
        .computers
        .get(&ctx.host)
        .is_some_and(|c| c.no_bar_indicator);
    let dir = ctx.home.join(PLUGIN_DIR).join(BAR_PLUGIN);
    let current = BAR_FILES
        .iter()
        .all(|(name, text)| std::fs::read_to_string(dir.join(name)).is_ok_and(|t| t == *text));
    // Only whether the shell's command is there: it answers `--version` with its usage and
    // exit code 1, so `has` would say it's missing on every Omarchy.
    let shell = Command::new("omarchy-shell")
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .is_ok();
    if off || current || !shell {
        return Ok(());
    }
    set_bar_indicator(ctx, true)
}

/// Puts the indicator on the bar, or takes it off, through Omarchy's own plugin commands.
pub fn set_bar_indicator(ctx: &Ctx, on: bool) -> Result<(), String> {
    let dir = ctx.home.join(PLUGIN_DIR).join(BAR_PLUGIN);
    if on {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // Older widget files go, so only this version's is left.
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !BAR_FILES.iter().any(|(n, _)| *n == name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
        for (name, text) in BAR_FILES {
            std::fs::write(dir.join(name), text).map_err(|e| e.to_string())?;
        }
        run(None, "omarchy-shell", &["shell", "rescanPlugins"])?;
        // The shell scans in the background, so the new plugin may take a moment to be known.
        let mut enabled = Err(String::new());
        for _ in 0..20 {
            enabled = run(None, "omarchy", &["plugin", "enable", BAR_PLUGIN]);
            if enabled.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        enabled?;
        // The shell caches each plugin folder's files from when it first read them, so a new
        // or changed widget file only loads after a restart (about a second; the bar redraws).
        run(None, "omarchy", &["restart", "shell"]).map(|_| ())
    } else {
        // Already off (or never on) is fine: the goal is that it's gone.
        let _ = run(None, "omarchy", &["plugin", "disable", BAR_PLUGIN]);
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        let _ = run(None, "omarchy-shell", &["shell", "rescanPlugins"]);
        Ok(())
    }
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
    /// Each clashing file as it is here and on the other computer, to compare and choose.
    pub versions: Vec<Versions>,
    /// Files (under home) that combined without a clash but came out broken, which is why they
    /// need a choice too.
    pub broken: Vec<String>,
}

/// One clashing file's two versions. `None` when that computer removed it; `binary` when its
/// contents can't be shown (not text, or too big).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Versions {
    /// Under home, for people.
    pub path: String,
    /// In the sync folder, for choosing (what `resolve` takes).
    pub source: String,
    pub mine: Option<String>,
    pub theirs: Option<String>,
    pub binary: bool,
}

/// Which version of a clashing file to keep.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub enum Keep {
    Mine,
    Theirs,
}

/// What a finished sync has to say: notes for the person, and apps waiting for their approval.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub notes: Vec<String>,
    pub waiting: Vec<String>,
}

/// Every computer writes its own entry in the shared choices file, so two computers syncing
/// close together often clash there without touching the same choice. While the shared file is
/// the only clash, each one is settled by merging the choices. A file switched off on either
/// side settles itself too (see `switched_off`). Returns false (still rebasing) when anything
/// else clashes.
fn settle_data(ctx: &Ctx) -> Result<bool, String> {
    const FILE: &str = ".chezmoidata.yaml";
    let mut switched_off_here = Vec::new();
    while ctx.rebasing() {
        let conflicted = ctx.conflicted();
        // During a rebase, stage 2 is what came from GitHub and stage 3 is this computer's.
        let side = |n: u8, file: &str| ctx.git(&["show", &format!(":{n}:{file}")]).ok();
        if conflicted.iter().any(|f| f == FILE) {
            let data = |n: u8| -> Data {
                side(n, FILE)
                    .and_then(|t| serde_yaml::from_str(&t).ok())
                    .unwrap_or_default()
            };
            ctx.save_data(&Data::merge(&data(1), &data(2), &data(3)))?;
            ctx.git(&["add", FILE])?;
        }
        let text = std::fs::read_to_string(ctx.data_path()).unwrap_or_default();
        let Ok(choices) = serde_yaml::from_str::<Data>(&text) else {
            return Ok(false);
        };
        for file in conflicted.iter().filter(|f| *f != FILE) {
            if !switched_off(ctx, &choices, file, ":2", ":3") {
                return Ok(false);
            }
            if side(3, file).is_none() {
                // This computer switched it off: the removal stands (the other computer's
                // version is in the history it sent).
                ctx.git(&["rm", "-q", "--", file])?;
            } else {
                // Another computer did: this computer's version stays in the history (the
                // replayed change keeps it), and the file leaves the sync folder in a change
                // of its own, below.
                ctx.git(&["checkout", "--theirs", "--", file])?;
                ctx.git(&["add", "--", file])?;
                switched_off_here.push(file.clone());
            }
        }
        // Settled to exactly what came from GitHub: nothing left of this change to replay.
        let nothing_left = ctx.git(&["diff", "--cached", "--quiet"]).is_ok();
        let go_on = if nothing_left { "--skip" } else { "--continue" };
        // Stops again at the next clashing change, if any; the loop looks again.
        if let Err(e) = ctx.git(&["-c", "core.editor=true", "rebase", go_on])
            && ctx.conflicted().is_empty()
        {
            return Err(e);
        }
    }
    if !switched_off_here.is_empty() {
        let mut args = vec!["rm", "-q", "--ignore-unmatch", "--"];
        args.extend(switched_off_here.iter().map(String::as_str));
        ctx.git(&args)?;
        if ctx.git(&["diff", "--cached", "--quiet"]).is_err() {
            ctx.git(&[
                "commit",
                "-q",
                "-m",
                &format!("Sync from {}", ctx.host),
                "-m",
                "Switched off on another computer; this computer keeps its own copy.",
            ])?;
        }
    }
    Ok(true)
}

/// A file changed on one computer and switched off on another isn't a clash: the switch
/// decides. One side took it out of the sync folder (`theirs` and `mine` are the two sides'
/// revisions, or their stages in a rebase), and the choices as merged no longer sync it here.
/// Every computer keeps the copy on its disk.
fn switched_off(ctx: &Ctx, choices: &Data, file: &str, theirs: &str, mine: &str) -> bool {
    let missing = |rev: &str| ctx.git(&["show", &format!("{rev}:{file}")]).is_err();
    !choices.syncs(&ctx.host, &source_to_target(file)) && (missing(theirs) || missing(mine))
}

/// Every file that clashes between this computer and GitHub, from a trial join of the two.
/// Replaying this computer's changes stops at the first one that clashes, which may not be the
/// only one; settling joins everything at once, so every clashing file needs a choice. Falls
/// back to `first` (the replay's) when the join finds none.
fn all_clashes(ctx: &Ctx, first: Vec<String>) -> Vec<String> {
    let _ = ctx.git(&["merge", "--no-ff", "--no-commit", "@{u}"]);
    if !ctx.merging() {
        return first;
    }
    let base = ctx
        .git(&["merge-base", "HEAD", "MERGE_HEAD"])
        .unwrap_or_default();
    let choices = Data::merge(
        &data_at(ctx, &base),
        &data_at(ctx, "MERGE_HEAD"),
        &data_at(ctx, "HEAD"),
    );
    let files: Vec<String> = ctx
        .conflicted()
        .into_iter()
        .filter(|f| {
            f != ".chezmoidata.yaml" && !switched_off(ctx, &choices, f, "MERGE_HEAD", "HEAD")
        })
        .collect();
    let _ = ctx.git(&["merge", "--abort"]);
    if files.is_empty() { first } else { files }
}

/// A clashing file as this computer has it and as GitHub has it, for showing side by side.
pub(crate) fn versions(ctx: &Ctx, source: &str) -> Versions {
    const LIMIT: usize = 256 * 1024;
    let read = |rev: &str| ctx.git(&["show", &format!("{rev}:{source}")]).ok();
    let (mine, theirs) = (read("HEAD"), read("@{u}"));
    let binary = [&mine, &theirs]
        .into_iter()
        .flatten()
        .any(|t| t.len() > LIMIT || t.contains('\0'));
    Versions {
        path: source_to_target(source),
        source: source.to_string(),
        mine: if binary { None } else { mine },
        theirs: if binary { None } else { theirs },
        binary,
    }
}

/// A clash to show: the files with both their versions, and the other computer that made the
/// newest change to them (from GitHub's side).
pub(crate) fn clash(ctx: &Ctx, files: Vec<String>, broken: Vec<String>) -> SyncError {
    let mut args = vec!["log", "-1", "--format=%an%x09%ct", "@{u}", "--"];
    args.extend(files.iter().map(|f| f.as_str()));
    let data = ctx.load_data();
    let by = ctx
        .git(&args)
        .ok()
        .and_then(|l| parse_log(&l).into_iter().next())
        .map(|(author, t)| (data.resolve(&author), t));
    let versions = files.iter().map(|f| versions(ctx, f)).collect();
    let targets = |paths: Vec<String>| paths.iter().map(|p| source_to_target(p)).collect();
    SyncError::Conflict(Conflict {
        files: targets(files),
        by,
        versions,
        broken: targets(broken),
    })
}

/// Whether a settings file still reads as its format: JSON, TOML, YAML or Lua (checked with
/// luac, where installed). Other files have no strict format, so they always pass.
pub fn well_formed(target: &str, text: &str) -> bool {
    match target.rsplit('.').next().unwrap_or_default() {
        "json" => serde_json::from_str::<serde_json::Value>(text).is_ok(),
        "toml" => text.parse::<toml::Table>().is_ok(),
        "yaml" | "yml" => serde_yaml::from_str::<serde_yaml::Value>(text).is_ok(),
        "lua" => {
            let file =
                std::env::temp_dir().join(format!("omarchy-sync-{}.lua", std::process::id()));
            if std::fs::write(&file, text).is_err() {
                return true;
            }
            let checked = Command::new("luac")
                .arg("-p")
                .arg(&file)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = std::fs::remove_file(&file);
            // No luac here: nothing to check with, so it passes.
            checked.map_or(true, |s| s.success())
        }
        _ => true,
    }
}

/// Files both computers changed since `before` parted from GitHub's history that git just
/// combined into something that no longer reads as their format, though each computer's own
/// version did (a file that was already broken on one side isn't combining's doing).
fn broken_by_combining(ctx: &Ctx, before: &str) -> Vec<String> {
    let Ok(base) = ctx.git(&["merge-base", before, "@{u}"]) else {
        return Vec::new();
    };
    let changed = |to: &str| -> BTreeSet<String> {
        ctx.git(&["diff", "--name-only", &base, to])
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    };
    let (here, there) = (changed(before), changed("@{u}"));
    here.intersection(&there)
        .filter(|f| *f != ".chezmoidata.yaml")
        .filter(|f| {
            let target = source_to_target(f);
            let reads = |rev: &str| {
                ctx.git(&["show", &format!("{rev}:{f}")])
                    .is_ok_and(|text| well_formed(&target, &text))
            };
            reads(before) && reads("@{u}") && !reads("HEAD")
        })
        .cloned()
        .collect()
}

/// Settles a clash with the person's choice for each clashing file, then syncs as usual.
/// Both computers' changes are joined (a merge), so each side's own version stays in the
/// history; every clashing file then becomes exactly the version chosen for it.
pub fn resolve(
    ctx: &Ctx,
    choices: &BTreeMap<String, Keep>,
    install: bool,
    step: &dyn Fn(&str),
) -> Result<Report, SyncError> {
    let fail = SyncError::Failed;
    let saved_first = {
        let _lock = lock_wait(ctx).map_err(fail)?;
        plan(
            step,
            &[
                "Saving this computer's changes",
                "Getting changes from your other computers",
                "Keeping the versions you chose",
            ],
        );
        step("Saving this computer's changes");
        save_local(ctx, &|_| {}).map_err(fail)?;
        let synced_before = ctx.load_data().active_folders(&ctx.host);
        step("Getting changes from your other computers");
        git_progress(ctx, &["fetch", "--progress"], step).map_err(fail)?;
        step("Keeping the versions you chose");
        // Joined without committing yet, so every chosen file (clashing or combined) can be set
        // to its chosen version first.
        let _ = ctx.git(&["merge", "--no-ff", "--no-commit", "@{u}"]);
        if ctx.merging() {
            let mut kept = Vec::new();
            let mut settled: Result<(), String> = settle_data_revs(ctx);
            for (source, keep) in choices {
                if settled.is_err() {
                    break;
                }
                settled = keep_version(ctx, source, *keep);
                let whose = if *keep == Keep::Mine {
                    "this computer's"
                } else {
                    "the other computer's"
                };
                kept.push(format!(
                    "{}: kept {whose} version",
                    label(&source_to_target(source))
                ));
            }
            if settled.is_ok() {
                let merged = ctx.load_data();
                for left in ctx
                    .conflicted()
                    .into_iter()
                    .filter(|f| f != ".chezmoidata.yaml" && !choices.contains_key(f))
                {
                    settled = if switched_off(ctx, &merged, &left, "MERGE_HEAD", "HEAD") {
                        ctx.git(&["rm", "-q", "--", &left]).map(|_| ())
                    } else {
                        Err(format!("no choice for {}", source_to_target(&left)))
                    };
                    if settled.is_err() {
                        break;
                    }
                }
            }
            if let Err(e) = settled {
                let _ = ctx.git(&["merge", "--abort"]);
                return Err(fail(e));
            }
            let message = format!(
                "Sync from {}\n\nSettled changes made on two computers.\n{}",
                ctx.host,
                kept.join("\n")
            );
            ctx.git(&["commit", "-q", "--no-edit", "-m", &message])
                .map_err(fail)?;
        }
        // Put the chosen versions in place now; the sync below starts by saving this computer's
        // files, which would otherwise save the clashing ones back over the choice.
        settle_pause(ctx).map_err(fail)?;
        let saved_first = save_newly_synced(ctx, &synced_before).map_err(fail)?;
        run(None, "chezmoi", &["apply", "--force"]).map_err(fail)?;
        saved_first
    };
    // Joined: the rest is an ordinary sync (apply here, share, apps and plugins).
    let mut report = sync(ctx, install, step)?;
    report.notes.splice(0..0, saved_first);
    Ok(report)
}

/// Makes a file exactly one side's version during a merge (this computer's is HEAD, the other's
/// MERGE_HEAD), whether git marked it as clashing or combined it; if that side removed it, it's
/// removed. (`checkout --ours` only touches files git marked as clashing.)
fn keep_version(ctx: &Ctx, source: &str, keep: Keep) -> Result<(), String> {
    let rev = if keep == Keep::Mine {
        "HEAD"
    } else {
        "MERGE_HEAD"
    };
    let from = format!("--source={rev}");
    if ctx
        .git(&["restore", &from, "--staged", "--worktree", "--", source])
        .is_ok()
    {
        ctx.git(&["add", "--", source]).map(|_| ())
    } else {
        ctx.git(&["rm", "-q", "--ignore-unmatch", "--", source])
            .map(|_| ())
    }
}

/// The shared choices file during a merge, always rebuilt from both computers' versions (git
/// may have combined it line by line, or clashed on it).
fn settle_data_revs(ctx: &Ctx) -> Result<(), String> {
    let base = ctx.git(&["merge-base", "HEAD", "MERGE_HEAD"])?;
    let read = |rev: &str| data_at(ctx, rev);
    ctx.save_data(&Data::merge(
        &read(&base),
        &read("MERGE_HEAD"),
        &read("HEAD"),
    ))?;
    ctx.git(&["add", ".chezmoidata.yaml"]).map(|_| ())
}

/// The shared choices as they were at a revision (empty where unreadable).
fn data_at(ctx: &Ctx, rev: &str) -> Data {
    ctx.git(&["show", &format!("{rev}:.chezmoidata.yaml")])
        .ok()
        .and_then(|t| serde_yaml::from_str(&t).ok())
        .unwrap_or_default()
}

/// The saved copy of this computer's Omarchy settings from before it took the shared ones.
const OMARCHY_BACKUP: &str = "before-other-omarchy-settings";

/// Pauses or resumes syncing Omarchy's own settings here, once the other computers' changes
/// are in and before they're applied. Resuming (after updating Omarchy, or to try the newer
/// settings) saves this computer's settings first; the shared ones then replace them.
fn settle_pause(ctx: &Ctx) -> Result<Option<String>, String> {
    let mut data = ctx.load_data();
    let newest = data.newest_omarchy();
    let pause = data.should_pause(&ctx.host);
    let Some(c) = data.computers.get_mut(&ctx.host) else {
        return Ok(None);
    };
    let before = c.clone();
    c.paused = pause;
    // Caught up: nothing left to try.
    if major(&c.omarchy) == newest {
        c.tried = 0;
    }
    if *c == before {
        return Ok(None);
    }
    let note = if before.paused && !pause {
        backup(ctx, &OMARCHY_OWN.map(String::from), OMARCHY_BACKUP)?;
        Some(
            "This computer now uses the Omarchy settings from your other computers. Its own were saved first."
                .to_string(),
        )
    } else {
        None
    };
    ctx.save_data(&data)?;
    ctx.git(&["commit", "-q", "-am", &format!("Sync from {}", ctx.host)])?;
    Ok(note)
}

/// Puts back the Omarchy settings saved before this computer tried a newer Omarchy's, and
/// pauses syncing them again.
pub fn put_back_omarchy(ctx: &Ctx) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    let file = ctx
        .home
        .join(".local/share/omarchy-sync/backups")
        .join(format!("{OMARCHY_BACKUP}.tar.gz"));
    let (f, h) = (file.to_string_lossy(), ctx.home.to_string_lossy());
    // The folders are only cleared once the saved copy is known to read.
    run(None, "tar", &["-tzf", &f])?;
    let mut data = ctx.load_data();
    data.computers.entry(ctx.host.clone()).or_default().tried = 0;
    let paused = data.should_pause(&ctx.host);
    data.computers.entry(ctx.host.clone()).or_default().paused = paused;
    ctx.save_data(&data)?;
    for own in OMARCHY_OWN {
        let _ = std::fs::remove_dir_all(ctx.home.join(own));
    }
    run(None, "tar", &["-xzf", &f, "-C", &h])?;
    // The shell keeps plugin code it has loaded (see the bar indicator).
    let _ = run(None, "omarchy", &["restart", "shell"]);
    Ok(())
}

/// After combining, the shared choices file must still read; if git's line-by-line combining
/// broke it, it's rebuilt from both computers' versions, never read as empty.
fn repair_data(ctx: &Ctx, before: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(ctx.data_path()).unwrap_or_default();
    if serde_yaml::from_str::<Data>(&text).is_ok() {
        return Ok(());
    }
    let base = ctx.git(&["merge-base", before, "@{u}"])?;
    ctx.save_data(&Data::merge(
        &data_at(ctx, &base),
        &data_at(ctx, "@{u}"),
        &data_at(ctx, before),
    ))?;
    ctx.git(&["commit", "-q", "-am", &format!("Sync from {}", ctx.host)])
        .map(|_| ())
}

/// A folder that starts syncing here (switched on elsewhere, or by an undo) comes in over this
/// computer's own copy, which was never saved while the folder wasn't syncing: it's saved
/// first. `synced_before` is `active_folders` from before the change came in.
fn save_newly_synced(ctx: &Ctx, synced_before: &[String]) -> Result<Option<String>, String> {
    let new_here: Vec<String> = ctx
        .load_data()
        .active_folders(&ctx.host)
        .into_iter()
        .filter(|f| !synced_before.contains(f) && ctx.home.join(f).exists())
        .collect();
    if new_here.is_empty() {
        return Ok(None);
    }
    backup(ctx, &new_here, &format!("before-syncing-{}", now()))?;
    let what: Vec<&str> = new_here.iter().map(|f| label(f)).collect();
    let verb = if what.len() == 1 { "syncs" } else { "sync" };
    Ok(Some(format!(
        "{} now {verb} here. This computer's own settings were saved first.",
        what.join(", ")
    )))
}

/// Syncs, and records how it went for the bar indicator (see `write_status`).
pub fn sync(ctx: &Ctx, install: bool, step: &dyn Fn(&str)) -> Result<Report, SyncError> {
    write_status(ctx, "syncing", "Syncing…");
    let result = sync_inner(ctx, install, step);
    match &result {
        Ok((_, installed)) => {
            let data = ctx.load_data();
            let waiting = missing_apps(&data, &ctx.host, installed, &repos_here()).len()
                + missing_scripted(&data, &ctx.host, installed).len()
                + data.missing_plugins(&ctx.host).len();
            if waiting > 0 {
                let what = plural(waiting, "app or plugin");
                write_status(ctx, "attention", &format!("{what} waiting for you"));
            } else {
                write_status(ctx, "synced", "Everything matches your other computers");
            }
        }
        Err(SyncError::Conflict(_)) => write_status(
            ctx,
            "attention",
            "The same setting changed on two computers: choose which to keep",
        ),
        Err(SyncError::Failed(e)) => write_status(ctx, "error", &friendly(e)),
    }
    result.map(|(report, _)| report)
}

/// Also returns the apps installed here afterwards (`pacman -Qq` takes a moment, so it's run
/// once per sync).
fn sync_inner(
    ctx: &Ctx,
    install: bool,
    step: &dyn Fn(&str),
) -> Result<(Report, BTreeSet<String>), SyncError> {
    let fail = SyncError::Failed;
    // One sync at a time: one already running (the background timer's) finishes first.
    let _lock = match lock(ctx) {
        Ok(l) => l,
        Err(_) => {
            step("Waiting for the sync already running");
            lock_wait(ctx).map_err(fail)?
        }
    };
    let mut notes = Vec::new();
    // A sync interrupted part way (a crash, the power going) leaves git mid-way; this one
    // starts afresh, and the interrupted one's changes are still on disk to save again.
    if ctx.rebasing() {
        let _ = ctx.git(&["rebase", "--abort"]);
    }
    if ctx.merging() {
        let _ = ctx.git(&["merge", "--abort"]);
    }

    plan(
        step,
        &[
            "Saving this computer's changes",
            "Saving changed settings",
            "Looking for new files",
            "Getting changes from your other computers",
            "Updating this computer",
            "Sharing with your other computers",
            // The page's look afterwards (overview with just_synced).
            "Checking your apps and plugins",
        ],
    );
    step("Saving this computer's changes");
    if let Some(name) = save_local(ctx, step).map_err(fail)? {
        notes.push(format!(
            "Another computer is already called {}, so this one is {name}. You can rename it in Settings.",
            ctx.hostname
        ));
    }

    step("Getting changes from your other computers");
    let synced_before = ctx.load_data().active_folders(&ctx.host);
    let tracked = ctx.git(&["rev-parse", "--abbrev-ref", "@{u}"]).is_ok();
    if tracked {
        git_progress(ctx, &["fetch", "--progress"], step).map_err(fail)?;
        // Already holding everything from GitHub (say, just after settling a clash, which joins
        // both computers' changes): nothing to replay, and replaying would bring the clash back.
        let caught_up = ctx
            .git(&["merge-base", "--is-ancestor", "@{u}", "HEAD"])
            .is_ok();
        let before = ctx.git(&["rev-parse", "HEAD"]).map_err(fail)?;
        let replayed = if caught_up {
            Ok(String::new())
        } else {
            ctx.git(&["rebase", "-q", "@{u}"])
        };
        if let Err(e) = replayed {
            if !ctx.rebasing() {
                return Err(fail(e));
            }
            if !settle_data(ctx).map_err(fail)? {
                let first = ctx.conflicted();
                let _ = ctx.git(&["rebase", "--abort"]);
                return Err(clash(ctx, all_clashes(ctx, first), Vec::new()));
            }
        }
        // Combined without a clash, but a file both computers changed may have come out
        // broken (a lost comma in JSON, say): then this computer goes back to where it was and
        // the person chooses, as for a clash.
        if !caught_up {
            let broken = broken_by_combining(ctx, &before);
            if !broken.is_empty() {
                ctx.git(&["reset", "-q", "--hard", &before]).map_err(fail)?;
                return Err(clash(ctx, broken.clone(), broken));
            }
            repair_data(ctx, &before).map_err(fail)?;
        }
    }

    step("Updating this computer");
    notes.extend(settle_pause(ctx).map_err(fail)?);
    notes.extend(save_newly_synced(ctx, &synced_before).map_err(fail)?);
    run(None, "chezmoi", &["apply", "--force"]).map_err(fail)?;
    let data = ctx.load_data();
    // The Current theme switch governs applying it too.
    if let Some(theme) = &data.theme
        && data.uses_theme(&ctx.host)
        && ctx.current_theme().as_ref() != Some(theme)
        && run(None, "omarchy", &["theme", "set", theme]).is_err()
    {
        notes.push(format!("The {theme} theme couldn't be switched on here."));
    }
    // After the theme, which brings its own background.
    if apply_background(ctx, &data).is_err() {
        notes.push("The synced background couldn't be shown here.".into());
    }

    step("Sharing with your other computers");
    if tracked {
        git_progress(ctx, &["push", "--progress"], step).map_err(fail)?;
    } else {
        git_progress(ctx, &["push", "--progress", "-u", "origin", "HEAD"], step).map_err(fail)?;
    }

    let plugins = data.missing_plugins(&ctx.host);
    if install && !plugins.is_empty() {
        step(&format!("Adding {}", plural(plugins.len(), "plugin")));
        match install_plugins(ctx, &plugins) {
            Ok(()) => notes.push(format!("Added {}.", plural(plugins.len(), "plugin"))),
            Err(e) => notes.push(format!("Couldn't add plugins. {}", friendly(&e))),
        }
    }

    let mut installed = installed();
    let missing = missing_apps(&data, &ctx.host, &installed, &repos_here());
    if missing.is_empty() || !install {
        return Ok((
            Report {
                notes,
                waiting: missing,
            },
            installed,
        ));
    }
    step(&format!("Installing {}", plural(missing.len(), "app")));
    match install_apps(&missing) {
        Ok(()) => {
            notes.push(format!("Installed {}.", missing.join(", ")));
            installed.extend(missing);
        }
        Err(e) => notes.push(format!(
            "Couldn't install {}. {}",
            missing.join(", "),
            friendly(&e)
        )),
    }
    Ok((
        Report {
            notes,
            waiting: vec![],
        },
        installed,
    ))
}

/// Stops syncing here: the timer goes off and the local sync folder is moved aside.
/// Settings on this computer stay exactly as they are. Returns where the folder went.
pub fn disconnect(ctx: &Ctx) -> Result<PathBuf, String> {
    let _lock = lock_wait(ctx)?;
    schedule(ctx, false)?;
    let dir = ctx.home.join(".local/share/omarchy-sync");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let moved = dir.join(format!("disconnected-{}", now()));
    std::fs::rename(&ctx.src, &moved).map_err(|e| e.to_string())?;
    Ok(moved)
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub sha: String,
    pub computer: String,
    pub when: i64,
    /// Plain names of what changed, e.g. "Keyboard shortcuts".
    pub labels: Vec<String>,
    pub is_undo: bool,
    /// Where two computers' changes were joined after a clash; it can't be undone on its own.
    pub is_merge: bool,
    /// The very first save, which has nothing before it to go back to.
    pub first: bool,
    /// Every file it changed: what Undo would put back.
    pub files: Vec<Change>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Change {
    /// The path under home, or DATA_LABEL for the shared choices.
    pub path: String,
    /// "Added", "Changed" or "Removed".
    pub how: &'static str,
}

/// Reads `git ... --name-status` lines ("M\tpath", "R100\told\tnew") as changes, skipping
/// the ignore rules, which aren't anyone's settings.
fn parse_changes<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<Change> {
    lines
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let status = parts.next()?;
            let file = parts.next_back()?;
            let how = match status.chars().next()? {
                'A' => "Added",
                'D' => "Removed",
                _ => "Changed",
            };
            (!file.starts_with(".chezmoiignore")).then(|| Change {
                path: source_to_target(file),
                how,
            })
        })
        .collect()
}

/// A commit message body naming every change in plain words, grouped like the app shows them.
fn describe(changes: &[Change]) -> String {
    let mut groups: Vec<(&str, Vec<String>)> = Vec::new();
    for c in changes {
        let name = label(&c.path);
        let file = c.path.rsplit('/').next().unwrap_or(&c.path);
        let item = if c.path == DATA_LABEL {
            "updated".to_string()
        } else {
            format!("{} {file}", c.how.to_lowercase())
        };
        match groups.iter_mut().find(|(n, _)| *n == name) {
            Some((_, items)) => items.push(item),
            None => groups.push((name, vec![item])),
        }
    }
    groups
        .iter()
        .map(|(name, items)| format!("{name}: {}", items.join(", ")))
        .collect::<Vec<_>>()
        .join("\n")
}

const UNDO_PREFIX: &str = "Undo: ";

pub fn history(ctx: &Ctx) -> Vec<Entry> {
    let log = ctx
        .git(&[
            "log",
            "-n",
            "50",
            "--name-status",
            "--format=%x1e%H%x09%an%x09%ct%x09%s%x09%P",
        ])
        .unwrap_or_default();
    parse_history(&log)
}

/// Parses `git log --name-status --format=%x1e%H%x09%an%x09%ct%x09%s%x09%P`.
pub fn parse_history(log: &str) -> Vec<Entry> {
    log.split('\x1e')
        .filter_map(|block| {
            let mut lines = block.lines();
            let head: Vec<&str> = lines.next()?.split('\t').collect();
            let [sha, computer, when, subject, parents] = head[..] else {
                return None;
            };
            let files = parse_changes(lines.filter(|l| !l.is_empty()));
            let mut labels: Vec<String> = Vec::new();
            for c in &files {
                let name = label(&c.path).to_string();
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
                is_merge: parents.contains(' '),
                first: parents.is_empty(),
                files,
            })
        })
        .collect()
}

/// chezmoi's source name for a file back to its path under home (dot_config -> .config).
pub fn source_to_target(path: &str) -> String {
    if path == ".chezmoidata.yaml" {
        return DATA_LABEL.into();
    }
    if path.starts_with(".background/") {
        return path.into();
    }
    path.split('/')
        .map(|part| {
            let p = strip_attributes(part);
            p.strip_prefix("dot_")
                .map_or(p.clone(), |r| format!(".{r}"))
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Reverses one earlier change. The caller then syncs, which applies it here and shares it.
pub fn undo(ctx: &Ctx, sha: &str) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    save_local(ctx, &|_| {})?;
    let synced_before = ctx.load_data().active_folders(&ctx.host);
    if ctx.git(&["revert", "--no-commit", sha]).is_err() {
        let _ = ctx.git(&["revert", "--abort"]);
        return Err("can't undo: later changes depend on it".into());
    }
    // Never puts back a file that's switched off now (the choices as undone decide): it would
    // reach every computer, over copies that were never saved.
    let choices = ctx.load_data();
    let staged = ctx.git(&["diff", "--cached", "--name-status"])?;
    let revives_switched_off = parse_changes(staged.lines()).iter().any(|c| {
        c.how == "Added"
            && c.path != DATA_LABEL
            && !c.path.starts_with(".background/")
            && !choices.syncs(&ctx.host, &c.path)
    });
    if revives_switched_off {
        let _ = ctx.git(&["revert", "--abort"]);
        return Err(
            "This change would put back settings that are switched off, so it can't be undone."
                .into(),
        );
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
    save_newly_synced(ctx, &synced_before)?;
    // Put the undone files in place now; otherwise the next sync would re-save the old ones over the undo.
    run(None, "chezmoi", &["apply", "--force"]).map(|_| ())
}

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
    let mut ids: BTreeSet<String> = ctx.local_plugins().into_iter().collect();
    ids.extend(data.plugins.iter().cloned());
    ids.extend(
        data.computers
            .values()
            .flat_map(|c| c.plugins.iter().cloned()),
    );
    let mut plugins: Vec<Plugin> = ids
        .iter()
        .filter(|id| is_plugin_id(id))
        .map(|id| ctx.plugin(id))
        .collect();
    plugins.sort_by_key(|p| p.name.to_lowercase());
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

/// Takes other computers' plugins onto this one: it now counts as having them, so the ignore
/// rules let them through, and they're written out straight away.
pub fn install_plugins(ctx: &Ctx, ids: &[String]) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    let mut data = ctx.load_data();
    let me = data.computers.entry(ctx.host.clone()).or_default();
    for id in ids {
        toggle(&mut me.plugins, id, true);
    }
    ctx.save_data(&data)?;
    let paths: Vec<String> = ids
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
    run(None, "chezmoi", &args).map(|_| ())
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
    if path.starts_with(".background/") {
        return "Background";
    }
    FOLDERS
        .iter()
        .filter(|(f, _, _)| path == *f || path.starts_with(&format!("{f}/")))
        .max_by_key(|(f, _, _)| f.len())
        .map_or(path, |(_, l, _)| l)
}

pub const REPO: &str = "omarchy-sync";

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
pub(crate) fn seed(ctx: &Ctx) -> Result<(), String> {
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
    stage(ctx, &data, &|_| {})?;
    commit(ctx)
}

/// Another computer already syncs: back up this one, then take the synced settings.
pub fn join(ctx: &Ctx, login: &str, step: &dyn Fn(&str)) -> Result<PathBuf, String> {
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
    adopt(ctx, &format!("https://github.com/{login}/{REPO}.git"), step)
}

pub(crate) fn adopt(ctx: &Ctx, url: &str, step: &dyn Fn(&str)) -> Result<PathBuf, String> {
    step("Downloading your synced settings");
    run(None, "chezmoi", &["init", url])?;
    let mut data = ctx.load_data();
    // Joining with an older Omarchy than the others: its own settings stay as they are.
    note_omarchy(&mut data, &ctx.host);
    let paused = data.should_pause(&ctx.host);
    data.computers.entry(ctx.host.clone()).or_default().paused = paused;
    ctx.save_data(&data)?;
    step("Backing up this computer's current settings");
    let backup = backup(ctx, &data.folders, &format!("before-first-sync-{}", now()))?;
    step("Updating this computer");
    run(None, "chezmoi", &["apply", "--force"])?;
    Ok(backup)
}

pub(crate) fn backup(ctx: &Ctx, folders: &[String], name: &str) -> Result<PathBuf, String> {
    let dir = ctx.home.join(".local/share/omarchy-sync/backups");
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
fn strip_attributes(name: &str) -> String {
    let mut name = name;
    while let Some(rest) = ATTRS.iter().find_map(|p| name.strip_prefix(p)) {
        name = rest;
    }
    name.to_string()
}

/// Shows the synced background here when it isn't already, through Omarchy's own command.
fn apply_background(ctx: &Ctx, data: &Data) -> Result<(), String> {
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

fn stage(ctx: &Ctx, data: &Data, step: &dyn Fn(&str)) -> Result<(), String> {
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
        return Ok(());
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
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect(),
        // Older chezmoi can't list them: add everything, slowly but completely.
        Err(_) => paths,
    };
    if new.is_empty() {
        return Ok(());
    }
    step(&format!("Saving {}", plural(new.len(), "new file")));
    let mut args = vec!["add"];
    args.extend(new.iter().map(|s| s.as_str()));
    run(None, "chezmoi", &args)?;
    prune_ignored(&ctx.src);
    Ok(())
}

/// Takes `.git` folders, `node_modules` and backup files out of the sync folder. The ignore
/// rules name them, but an exception (a shared plugin, the current theme) wins over every
/// ignore rule in chezmoi, so whatever those folders hold gets added.
fn prune_ignored(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let path = e.path();
        if path.is_dir() {
            if name == "dot_git" || name == "node_modules" {
                let _ = std::fs::remove_dir_all(&path);
            } else if name != ".git" {
                prune_ignored(&path);
            }
        } else if strip_attributes(&name).contains(".bak.") {
            let _ = std::fs::remove_file(&path);
        }
    }
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
    prune_web_apps(ctx, &data);
    ctx.save_data(&data)?;
    stage(ctx, &data, step)?;
    commit(ctx)?;
    Ok(renamed)
}

/// Records this computer's Omarchy version.
fn note_omarchy(data: &mut Data, host: &str) {
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
            run(None, "chezmoi", &["add", &path])?;
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
        let args = if on {
            vec!["add", path.as_str()]
        } else {
            vec!["forget", "--force", path.as_str()]
        };
        run(None, "chezmoi", &args)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_from_outside_the_repos_install_only_by_script() {
        assert!(is_package_name("numpad") && is_package_name("lib32-gcc+x@1.2"));
        assert!(!is_package_name("-rf") && !is_package_name("a b") && !is_package_name("A"));
        let mut d = Data {
            apps: vec!["numpad".into(), "rustxt".into(), "firefox".into()],
            ..Default::default()
        };
        d.outside.insert("numpad".into(), None);
        let script = Script {
            command: "curl -fsSL https://example.com/install.sh | bash".into(),
            ..Default::default()
        };
        d.outside.insert("rustxt".into(), Some(script.clone()));
        let none = BTreeSet::new();
        // yay only gets what it can install; rustxt waits for its script; numpad can't come.
        assert_eq!(
            missing_apps(&d, "b", &none, &none),
            vec!["firefox".to_string()]
        );
        assert_eq!(
            missing_scripted(&d, "b", &none),
            vec![("rustxt".to_string(), script.clone())]
        );
        // A script added on one computer and an app found on another both survive a merge.
        let base = d.clone();
        let mut mine = d.clone();
        mine.outside.insert("numpad".into(), Some(script.clone()));
        let mut theirs = d.clone();
        theirs.outside.insert("owl".into(), None);
        let m = Data::merge(&base, &theirs, &mine);
        assert_eq!(m.outside.get("numpad"), Some(&Some(script)));
        assert_eq!(m.outside.get("owl"), Some(&None));
    }

    #[test]
    fn missing_apps_only_counts_wanted_and_absent() {
        let mut d = Data {
            apps: vec!["a".into(), "b".into(), "c".into()],
            ..Default::default()
        };
        d.set_app("pc", "c", false);
        let installed: BTreeSet<String> = ["a".to_string()].into();
        let none = BTreeSet::new();
        assert_eq!(
            missing_apps(&d, "pc", &installed, &none),
            vec!["b".to_string()]
        );
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
    fn apps_from_an_extra_repository_wait_for_it() {
        let mut d = Data {
            apps: vec!["chatgpt-bin".into(), "firefox".into()],
            ..Default::default()
        };
        d.app_repo
            .insert("chatgpt-bin".into(), "openai-chatgpt".into());
        d.repositories.insert(
            "openai-chatgpt".into(),
            Repository {
                servers: vec!["https://example.com/arch".into()],
                ..Default::default()
            },
        );
        let none = BTreeSet::new();
        let missing = missing_repos(&d, "b", &none, &BTreeSet::from(["core".into()]));
        assert_eq!(
            (
                missing.len(),
                missing[0].name.as_str(),
                missing[0].apps.clone()
            ),
            (1, "openai-chatgpt", vec!["chatgpt-bin".to_string()])
        );
        // Once the repository is here, nothing waits for it.
        let here = BTreeSet::from(["openai-chatgpt".to_string()]);
        assert!(missing_repos(&d, "b", &none, &here).is_empty());
        assert_eq!(repos_with_apps(&d, &here)[0].apps, missing[0].apps);
        // A repository recorded on one computer survives a merge with another's changes.
        let base = Data::default();
        let theirs = Data {
            apps: vec!["yazi".into()],
            ..Default::default()
        };
        let m = Data::merge(&base, &theirs, &d);
        assert!(m.repositories.contains_key("openai-chatgpt"));
        assert_eq!(
            m.app_repo.get("chatgpt-bin").map(String::as_str),
            Some("openai-chatgpt")
        );
    }

    #[test]
    fn a_tampered_repository_is_refused_before_anything_runs() {
        let mut d = Data::default();
        let repo = |server: &str, level: &str| Repository {
            servers: vec![server.into()],
            sig_level: vec![level.into()],
            key: None,
        };
        d.repositories
            .insert("plain-http".into(), repo("http://example.com", "Required"));
        d.repositories.insert(
            "odd-level".into(),
            repo("https://example.com", "Required; rm -rf"),
        );
        d.repositories
            .insert("bad name!".into(), repo("https://example.com", "Required"));
        d.repositories
            .insert("core".into(), repo("https://example.com", "Required"));
        for name in ["plain-http", "odd-level", "bad name!", "core", "unknown"] {
            assert!(
                add_repository(&d, name).is_err(),
                "{name} should be refused"
            );
        }
    }

    #[test]
    fn the_background_sync_speaks_up_once_per_thing() {
        let mut state = BTreeMap::new();
        let failed = Outcome::Failed("git fetch: could not resolve host".into());
        // One failure is a blip; the second gets a word; the third is the same news.
        assert!(tell(&mut state, &failed).is_none());
        let (title, body) = tell(&mut state, &failed).unwrap();
        assert_eq!(title, "Syncing isn't working");
        assert!(body.starts_with("Couldn't reach GitHub"));
        assert!(tell(&mut state, &failed).is_none());
        // A clash once, until it's settled.
        assert!(tell(&mut state, &Outcome::Clash).is_some());
        assert!(tell(&mut state, &Outcome::Clash).is_none());
        // Back in order: nothing to say, and the counters start over.
        let quiet = Outcome::Synced {
            apps: vec![],
            plugins: vec![],
        };
        assert!(tell(&mut state, &quiet).is_none());
        assert!(!state.contains_key("failures") && !state.contains_key("conflict"));
        assert!(tell(&mut state, &failed).is_none());
        assert!(tell(&mut state, &Outcome::Clash).is_some());
        // The same waiting list is one notification; a changed one is another.
        let waiting = |apps: &[&str], plugins: &[&str]| Outcome::Synced {
            apps: apps.iter().map(|s| s.to_string()).collect(),
            plugins: plugins.iter().map(|s| s.to_string()).collect(),
        };
        let (title, body) = tell(&mut state, &waiting(&["zed"], &[])).unwrap();
        assert_eq!(
            (title.as_str(), body.as_str()),
            (
                "Apps are ready to install",
                "zed. Open Omarchy Sync to add them."
            )
        );
        assert!(tell(&mut state, &waiting(&["zed"], &[])).is_none());
        let (title, _) = tell(&mut state, &waiting(&["zed"], &["Dock"])).unwrap();
        assert_eq!(title, "Apps and plugins are ready to add");
        assert_eq!(
            tell(&mut state, &waiting(&[], &["Dock"])).unwrap().0,
            "Plugins are ready to add"
        );
        // Once added, a quiet sync says nothing, and the next wait is news again.
        assert!(tell(&mut state, &quiet).is_none());
        assert!(tell(&mut state, &waiting(&["zed"], &[])).is_some());
    }

    #[test]
    fn settings_files_are_checked_by_format() {
        assert!(well_formed("a.json", r#"{"a": 1}"#));
        assert!(!well_formed("a.json", r#"{"a": 1,}"#));
        assert!(well_formed("a.toml", "a = 1"));
        assert!(!well_formed("a.toml", "a = 1\na = 2"));
        assert!(well_formed("a.yaml", "a: 1"));
        assert!(!well_formed("a.yaml", "a: [1"));
        assert!(well_formed("ghostty/config", "anything = goes"));
        // Lua is checked with luac where it's installed.
        if Command::new("luac").arg("-v").output().is_ok() {
            assert!(well_formed("a.lua", "local a = 1"));
            assert!(!well_formed("a.lua", "local a = "));
        }
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
        let log = "\x1eaaa\tlaptop\t200\tUndo: a change from desk\tbbb\n\nM\tdot_config/hypr/bindings.lua\nM\t.chezmoidata.yaml\n\x1ebbb\tdesk\t100\tSync from desk\t\n\nA\tdot_config/hypr/input.lua\nD\tdot_config/hypr/bindings.lua\n";
        let h = parse_history(log);
        assert_eq!(h.len(), 2);
        assert_eq!(
            h[0].labels,
            vec!["Keyboard shortcuts".to_string(), DATA_LABEL.to_string()]
        );
        assert!(h[0].is_undo && !h[0].first);
        assert_eq!(
            (
                h[1].computer.as_str(),
                h[1].when,
                h[1].first,
                h[1].labels.len()
            ),
            ("desk", 100, true, 2)
        );
        assert_eq!(
            h[1].files,
            vec![
                Change {
                    path: ".config/hypr/input.lua".into(),
                    how: "Added"
                },
                Change {
                    path: ".config/hypr/bindings.lua".into(),
                    how: "Removed"
                },
            ]
        );
        assert_eq!(
            describe(&h[0].files),
            format!("Keyboard shortcuts: changed bindings.lua\n{DATA_LABEL}: updated")
        );
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

#[cfg(test)]
mod lab;
#[cfg(test)]
mod scenarios;
