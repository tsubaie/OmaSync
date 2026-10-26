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
    here().repos_here()
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

/// This computer's extra repositories as its package manager uses them, with the key that
/// signs each, and which of the person's apps come from one.
pub(crate) fn record_repositories(data: &mut Data) {
    let platform = here();
    let extra: Vec<String> = repos_here()
        .into_iter()
        .filter(|r| !platform.standard_repos().contains(&r.as_str()))
        .collect();
    if extra.is_empty() {
        return;
    }
    for name in &extra {
        if let Some(repository) = platform
            .read_repository(name)
            .filter(|r| !r.servers.is_empty())
        {
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
pub(crate) fn snapshot_from(number: &str, err: &str) -> SnapshotOutcome {
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

/// A shell word holding exactly `s`.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Adds an extra repository on this computer, as recorded from another one (see
/// `PackageAdapter::repository_script`), after a snapshot of the system. One password prompt
/// (pkexec); every value is checked first.
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
    let platform = here();
    if !plain(name) || platform.standard_repos().contains(&name) {
        return Err(format!("not a repository that can be added: {name}"));
    }
    if repos_here().contains(name) {
        return Ok(SnapshotOutcome::Unavailable);
    }
    let tmp = TempDir::new("repo")?;
    let dir = tmp.path();
    let lines = platform.repository_script(name, repo, dir)?;
    let snapper = platform.snapshots_available();
    let mut script = String::from("set -e\n");
    if snapper {
        let label = format!("Before Omarchy Sync added repository {name}");
        script.push_str(&snapshot_lines(&label, &dir.join("snapshot"), ""));
    }
    script.push_str(&lines);
    std::fs::write(dir.join("add.sh"), &script).map_err(|e| e.to_string())?;
    run_all(&platform.as_root("bash", &[&dir.join("add.sh").to_string_lossy()]))?;
    Ok(if snapper {
        read_snapshot(&dir.join("snapshot"))
    } else {
        SnapshotOutcome::Unavailable
    })
}

/// Runs a command given as its words.
fn run_all(cmd: &[String]) -> Result<String, String> {
    let args: Vec<&str> = cmd[1..].iter().map(String::as_str).collect();
    run(None, &cmd[0], &args)
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

/// Where apps come from (see `PackageAdapter::origins`); apps from no repository install with
/// their steps. An app from an extra
/// repository this computer doesn't have (`app_repo`, recorded by another computer) shows that
/// repository rather than the AUR.
pub fn sources<'a>(
    apps: impl Iterator<Item = &'a String>,
    outside_here: &[String],
    outside_synced: &BTreeMap<String, Option<Script>>,
    app_repo: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let platform = here();
    let origin = platform.origins();
    apps.map(|app| {
        let source = match origin.get(app) {
            Some(repo) => repo.as_str(),
            None if outside_here.contains(app) || outside_synced.contains_key(app) => "elsewhere",
            None if app_repo.contains_key(app) => app_repo[app].as_str(),
            None => platform.unlisted_origin(),
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
    let snapper = here().snapshots_available();
    let snapshot = if snapper {
        let label = format!("Before Omarchy Sync ran install steps for {app}");
        format!(
            "echo 'Taking a snapshot of this computer'\\''s system first.'\n{}",
            snapshot_lines(&label, &taken, here().terminal_root())
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
    let snapshot = here().snapshot(&format!(
        "Before Omarchy Sync installed {}",
        apps.join(", ")
    ));
    if snapshot == SnapshotOutcome::Cancelled {
        return Err("pkexec snapper: Request dismissed".into());
    }
    install_packages(apps)?;
    Ok(snapshot)
}

/// Installs packages (on Omarchy through yay), asking for the password itself (pkexec).
pub fn install_packages(apps: &[String]) -> Result<(), String> {
    run_all(&here().install_command(apps)).map(|_| ())
}

pub fn installed() -> BTreeSet<String> {
    here().installed()
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
    let explicit = here().explicit();
    user_apps(explicit.iter().map(String::as_str), &base)
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
    let (about, mut website) = here().info(&names);
    // Only a web address: a package may name anything here.
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
