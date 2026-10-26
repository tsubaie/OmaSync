//! Apps: what's installed here, where each comes from, extra repositories, and installing.

use super::*;

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

/// Extra repositories this computer lacks for synced apps it wants: ones it doesn't have yet,
/// and ones it has that stopped updating when the repository went (an Omarchy upgrade can
/// rewrite pacman.conf).
pub fn missing_repos(data: &Data, host: &str, here: &BTreeSet<String>) -> Vec<MissingRepo> {
    let mut needed: BTreeMap<&String, Vec<String>> = BTreeMap::new();
    for app in data.apps.iter().filter(|a| data.wants_app(host, a)) {
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
pub(crate) fn record_repositories(data: &mut Data) {
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
    let tmp = TempDir::new("keys").ok()?;
    let dir = tmp.path();
    let ring = ["pubring.gpg", "pubring.kbx"]
        .iter()
        .find(|f| std::fs::copy(Path::new("/etc/pacman.d/gnupg").join(f), dir.join(f)).is_ok());
    let home = dir.to_string_lossy().into_owned();
    let gpg = |args: &[&str]| {
        let mut all = vec!["--homedir", home.as_str(), "--no-autostart", "--batch"];
        all.extend_from_slice(args);
        run(None, "gpg", &all).ok()
    };
    ring.and_then(|_| {
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
    })
}

/// How the snapshot of this computer's system (Snapper's `root` config: packages and /etc, not
/// home) went before a change to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapshotOutcome {
    /// Its number, which the startup menu shows.
    Taken(u32),
    /// No Snapper here, or no config for the system: nothing is said.
    Unavailable,
    /// The change still goes ahead, and the person is told.
    Failed(String),
    /// The password window was closed: the change doesn't go ahead either.
    Cancelled,
}

/// Whether Snapper can snapshot the system here. The lab points the config elsewhere.
pub fn snapper_available() -> bool {
    let config = std::env::var_os("OMARCHY_SYNC_SNAPPER_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/etc/snapper/configs/root".into());
    config.exists() && has("snapper")
}

/// A snapshot's number from what `snapper --print-number` printed, or why there's none.
fn snapshot_from(number: &str, err: &str) -> SnapshotOutcome {
    match number.trim().parse() {
        Ok(n) => SnapshotOutcome::Taken(n),
        Err(_) if !err.trim().is_empty() => SnapshotOutcome::Failed(err.trim().to_string()),
        Err(_) => SnapshotOutcome::Failed("snapper didn't say which snapshot it made".into()),
    }
}

/// Script lines that snapshot the system, as root (`sudo` is "" or "sudo "), writing its number
/// to `out` and any error beside it, then let Snapper clean up old ones. They never stop the
/// script: the change goes ahead without a snapshot.
fn snapshot_lines(label: &str, out: &Path, sudo: &str) -> String {
    format!(
        "if {sudo}snapper -c root create -c number -d {} --print-number > {} 2> {}; then {sudo}snapper -c root cleanup number > /dev/null 2>&1 || true; fi\n",
        quote(label),
        quote(&out.to_string_lossy()),
        quote(&out.with_extension("err").to_string_lossy()),
    )
}

/// What the lines from `snapshot_lines` left in `out`.
fn read_snapshot(out: &Path) -> SnapshotOutcome {
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    snapshot_from(&read(out), &read(&out.with_extension("err")))
}

/// Snapshots the system through its own password prompt (pkexec), when Snapper is here.
fn snapshot(label: &str) -> SnapshotOutcome {
    if !snapper_available() {
        return SnapshotOutcome::Unavailable;
    }
    // Cleanup is its own step, so its failure never hides a snapshot that was made.
    let script = "snapper -c root create -c number -d \"$1\" --print-number || exit 2
snapper -c root cleanup number > /dev/null 2>&1
exit 0";
    let out = Command::new("pkexec")
        .args(["sh", "-c", script, "sh", label])
        .stdin(Stdio::null())
        .env("LC_ALL", "C.UTF-8")
        .output();
    match out {
        Err(e) => SnapshotOutcome::Failed(format!("pkexec: {e}")),
        // pkexec's own codes: the window was closed, or the password not given.
        Ok(o) if matches!(o.status.code(), Some(126 | 127)) => SnapshotOutcome::Cancelled,
        Ok(o) => snapshot_from(
            &String::from_utf8_lossy(&o.stdout),
            &String::from_utf8_lossy(&o.stderr),
        ),
    }
}

/// A shell word holding exactly `s`.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Adds an extra repository on this computer, as recorded from another one: its settings file
/// in /etc/pacman.d, an Include for it in pacman.conf, its key imported and trusted, and the
/// package lists refreshed, after a snapshot of the system. One password prompt (pkexec); every
/// value is checked first.
pub fn add_repository(data: &Data, name: &str) -> Result<SnapshotOutcome, String> {
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
        return Ok(SnapshotOutcome::Unavailable);
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
    let tmp = TempDir::new("repo")?;
    let dir = tmp.path();
    let mut conf = format!("[{name}]\n");
    if !repo.sig_level.is_empty() {
        conf.push_str(&format!("SigLevel = {}\n", repo.sig_level.join(" ")));
    }
    for server in &repo.servers {
        conf.push_str(&format!("Server = {server}\n"));
    }
    let target = format!("/etc/pacman.d/omarchy-sync-{name}.conf");
    let include = format!("Include = {target}");
    let snapper = snapper_available();
    let mut script = String::from("set -e\n");
    if snapper {
        let label = format!("Before Omarchy Sync added repository {name}");
        script.push_str(&snapshot_lines(&label, &dir.join("snapshot"), ""));
    }
    script.push_str(&format!(
        "install -m 644 '{}' '{target}'\ngrep -qxF '{include}' /etc/pacman.conf || printf '\\n%s\\n' '{include}' >> /etc/pacman.conf\n",
        dir.join("repo.conf").display()
    ));
    std::fs::write(dir.join("repo.conf"), conf).map_err(|e| e.to_string())?;
    if let Some(key) = &repo.key {
        let fpr_ok =
            key.fingerprint.len() == 40 && key.fingerprint.bytes().all(|b| b.is_ascii_hexdigit());
        if !fpr_ok
            || !key
                .armored
                .starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----")
        {
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
    run(
        None,
        "pkexec",
        &["bash", &dir.join("add.sh").to_string_lossy()],
    )?;
    Ok(if snapper {
        read_snapshot(&dir.join("snapshot"))
    } else {
        SnapshotOutcome::Unavailable
    })
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
/// so the person sees what it does and can type a password if it asks, after a snapshot of the
/// system (through sudo, in the same window). Waits for the command to finish, then reports how
/// it ended.
pub fn install_with_script(
    ctx: &Ctx,
    app: &str,
    script: &Script,
    step: &dyn Fn(&str),
) -> Result<SnapshotOutcome, String> {
    // The name comes from the shared file, which anyone can edit on GitHub: only a plain
    // package name becomes a folder that is emptied below.
    if !is_package_name(app) {
        return Err(format!("not an app: {app}"));
    }
    let dir = ctx.home.join(".cache/omarchy-sync/install").join(app);
    let done = dir.with_extension("done");
    let taken = dir.with_extension("snapshot");
    let _ = std::fs::remove_dir_all(&dir);
    for f in [&done, &taken, &taken.with_extension("err")] {
        let _ = std::fs::remove_file(f);
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    step(&format!("Installing {app} in a terminal window"));
    let snapper = snapper_available();
    let snapshot = if snapper {
        let label = format!("Before Omarchy Sync ran install steps for {app}");
        format!(
            "echo 'Taking a snapshot of this computer'\\''s system first.'\n{}",
            snapshot_lines(&label, &taken, "sudo ")
        )
    } else {
        String::new()
    };
    // The steps run as one script that stops at the first failing step, so a failed build
    // never goes on to install.
    let file = dir.with_extension("sh");
    std::fs::write(
        &file,
        format!(
            "set -e -o pipefail\n{snapshot}cd {}\n{}\n",
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
                "0" if snapper => Ok(read_snapshot(&taken)),
                "0" => Ok(SnapshotOutcome::Unavailable),
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

/// Installs synced apps through yay, after a snapshot of the system. Closing the snapshot's
/// password window installs nothing.
pub fn install_apps(apps: &[String]) -> Result<SnapshotOutcome, String> {
    let snapshot = snapshot(&format!(
        "Before Omarchy Sync installed {}",
        apps.join(", ")
    ));
    if snapshot == SnapshotOutcome::Cancelled {
        return Err("pkexec snapper: Request dismissed".into());
    }
    install_packages(apps)?;
    Ok(snapshot)
}

/// Installs packages through yay, which asks for the password itself (pkexec).
pub fn install_packages(apps: &[String]) -> Result<(), String> {
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
pub(crate) fn user_apps_here() -> Vec<String> {
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
    let info = if names.is_empty() {
        String::new()
    } else {
        let mut args = vec!["-Qi"];
        args.extend(names);
        run(None, "pacman", &args).unwrap_or_default()
    };
    let about = parse_info(&info, "Description");
    // Only a web address: a package may name anything here.
    let mut website = parse_info(&info, "URL");
    website.retain(|_, url| is_git_address(url));
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
        website,
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
        let missing = missing_repos(&d, "b", &BTreeSet::from(["core".into()]));
        assert_eq!(
            (
                missing.len(),
                missing[0].name.as_str(),
                missing[0].apps.clone()
            ),
            (1, "openai-chatgpt", vec!["chatgpt-bin".to_string()])
        );
        // Once the repository is here, nothing waits for it. (An installed app whose repository
        // went still does: it can't update until the repository is back.)
        let here = BTreeSet::from(["openai-chatgpt".to_string()]);
        assert!(missing_repos(&d, "b", &here).is_empty());
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
}
