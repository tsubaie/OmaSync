//! What differs between Linux distributions: the package manager (`PackageAdapter`), the
//! desktop (`DesktopAdapter`: Omarchy's themes, bar and plugins) and running things as root
//! (`PrivilegeAdapter`). `here()` picks the adapter for this computer from /etc/os-release;
//! Omarchy (and the lab, which fakes it) always gets `OmarchyArchAdapter`.

use super::*;

/// A family of distributions sharing a package manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DistroKind {
    Arch,
    Debian,
    Fedora,
}

/// What the desktop here can do with the shared settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DistroProfile {
    pub supports_themes: bool,
    pub supports_bar_indicator: bool,
    pub supports_plugins: bool,
    /// Omarchy's own settings pause on a computer behind on Omarchy's major version.
    pub supports_major_version_guard: bool,
}

impl DistroProfile {
    pub const OMARCHY: DistroProfile = DistroProfile {
        supports_themes: true,
        supports_bar_indicator: true,
        supports_plugins: true,
        supports_major_version_guard: true,
    };

    /// Any other desktop: only the apps' own settings sync.
    pub const PLAIN: DistroProfile = DistroProfile {
        supports_themes: false,
        supports_bar_indicator: false,
        supports_plugins: false,
        supports_major_version_guard: false,
    };
}

/// Installing, listing and finding packages and their repositories.
pub trait PackageAdapter {
    /// Every package installed here.
    fn installed(&self) -> BTreeSet<String>;

    /// The packages installed on purpose, not as another's dependency.
    fn explicit(&self) -> Vec<String>;

    /// Each installed package's one-line description and website, by name.
    fn info(&self, names: &[&str]) -> (BTreeMap<String, String>, BTreeMap<String, String>);

    /// Where each package comes from, as `Apps::source` names it.
    fn origins(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    /// The source of a package `origins` doesn't list.
    fn unlisted_origin(&self) -> &'static str;

    /// Repositories every computer of this kind has; only others are recorded and offered.
    fn standard_repos(&self) -> &'static [&'static str];

    /// The package repositories this computer uses.
    fn repos_here(&self) -> BTreeSet<String>;

    /// An extra repository as this computer uses it, to add on another.
    fn read_repository(&self, name: &str) -> Option<Repository>;

    /// Root script lines that add a recorded repository and refresh the package lists, after
    /// writing the files they install into `dir`. Every value is checked first.
    fn repository_script(
        &self,
        name: &str,
        repo: &Repository,
        dir: &Path,
    ) -> Result<String, String>;

    /// The command that installs packages, asking for the password itself.
    fn install_command(&self, apps: &[String]) -> Vec<String>;

    /// The command that installs setup's tools (`chezmoi`, `gh`, `git`) under this
    /// distribution's package names, asking for the password itself.
    fn helpers_command(&self, need: &[&str]) -> Vec<String>;

    fn install_helpers(&self, need: &[&str]) -> Result<(), String> {
        let cmd = self.helpers_command(need);
        let args: Vec<&str> = cmd[1..].iter().map(String::as_str).collect();
        run(None, &cmd[0], &args).map(|_| ())
    }

    /// The package that owns a file.
    fn owner(&self, path: &str) -> Option<String>;
}

/// The desktop's own commands. Each is used only when the profile says it's there; the
/// defaults are for a desktop without them.
pub trait DesktopAdapter {
    fn profile(&self) -> DistroProfile;

    /// The desktop's version, to compare major versions with other computers.
    fn desktop_version(&self) -> Option<String> {
        None
    }

    fn set_theme(&self, _name: &str) -> Result<(), String> {
        Err(no_omarchy("themes"))
    }

    fn set_background(&self, _image: &Path) -> Result<(), String> {
        Err(no_omarchy("backgrounds"))
    }

    /// `omarchy plugin …`.
    fn plugin(&self, _args: &[&str]) -> Result<String, String> {
        Err(no_omarchy("plugins"))
    }

    /// Whether the shell that draws the bar is here.
    fn has_shell(&self) -> bool {
        false
    }

    /// `omarchy-shell …`.
    fn shell(&self, _args: &[&str]) -> Result<String, String> {
        Err(no_omarchy("the bar"))
    }

    fn restart_shell(&self) -> Result<(), String> {
        Ok(())
    }
}

fn no_omarchy(what: &str) -> String {
    format!("{what} need Omarchy")
}

/// Running things as root, and the snapshot of the system taken before a change to it.
pub trait PrivilegeAdapter {
    /// `prog args` as root, behind the desktop's password window.
    fn as_root(&self, prog: &str, args: &[&str]) -> Vec<String> {
        ["pkexec", prog]
            .into_iter()
            .chain(args.iter().copied())
            .map(String::from)
            .collect()
    }

    /// What starts a root line in a script run in a terminal window, where the person types
    /// the password.
    fn terminal_root(&self) -> &'static str {
        "sudo "
    }

    fn snapshots_available(&self) -> bool {
        snapper_available()
    }

    /// Snapshots the system through its own password prompt, when Snapper is here.
    fn snapshot(&self, label: &str) -> SnapshotOutcome {
        if !self.snapshots_available() {
            return SnapshotOutcome::Unavailable;
        }
        // Cleanup is its own step, so its failure never hides a snapshot that was made.
        let script = "snapper -c root create -c number -d \"$1\" --print-number || exit 2
snapper -c root cleanup number > /dev/null 2>&1
exit 0";
        let cmd = self.as_root("sh", &["-c", script, "sh", label]);
        let out = Command::new(&cmd[0])
            .args(&cmd[1..])
            .stdin(Stdio::null())
            .env("LC_ALL", "C.UTF-8")
            .output();
        match out {
            Err(e) => SnapshotOutcome::Failed(format!("{}: {e}", cmd[0])),
            // pkexec's own codes: the window was closed, or the password not given.
            Ok(o) if matches!(o.status.code(), Some(126 | 127)) => SnapshotOutcome::Cancelled,
            Ok(o) => snapshot_from(
                &String::from_utf8_lossy(&o.stdout),
                &String::from_utf8_lossy(&o.stderr),
            ),
        }
    }
}

/// All three, for one computer.
pub trait Platform: PackageAdapter + DesktopAdapter + PrivilegeAdapter + Sync {}

impl<T: PackageAdapter + DesktopAdapter + PrivilegeAdapter + Sync> Platform for T {}

/// The adapter for this computer. Looked up each time (it's a file read): the lab changes
/// HOME and PATH between scenarios.
pub fn here() -> &'static dyn Platform {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let omarchy = home.join(".local/share/omarchy").is_dir() || on_path("omarchy");
    detect(
        &std::fs::read_to_string("/etc/os-release").unwrap_or_default(),
        omarchy,
    )
}

fn on_path(prog: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
}

/// The adapter for an /etc/os-release, and whether Omarchy is installed.
pub fn detect(os_release: &str, omarchy: bool) -> &'static dyn Platform {
    match parse_os_release(os_release) {
        _ if omarchy => &OmarchyArchAdapter,
        Some(DistroKind::Debian) => &AptAdapter,
        Some(DistroKind::Fedora) => &DnfAdapter,
        // Arch, and anything unknown: the app was made for Omarchy, so as it always did.
        Some(DistroKind::Arch) | None => &OmarchyArchAdapter,
    }
}

/// The distribution family `ID` names, or else the first of `ID_LIKE` known here.
pub fn parse_os_release(text: &str) -> Option<DistroKind> {
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('='))
            .map(|v| v.trim().trim_matches(['"', '\'']).to_lowercase())
            .unwrap_or_default()
    };
    let (id, like) = (field("ID"), field("ID_LIKE"));
    std::iter::once(id.as_str())
        .chain(like.split_whitespace())
        .find_map(|id| match id {
            "arch" | "archarm" | "omarchy" => Some(DistroKind::Arch),
            "debian" | "ubuntu" => Some(DistroKind::Debian),
            "fedora" | "rhel" | "centos" => Some(DistroKind::Fedora),
            _ => None,
        })
}

/// Lines of a command's output, without empty ones.
fn lines(out: &str) -> impl Iterator<Item = String> + '_ {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
}

/// The key that signs a repository, from the person's own computer: 40 hex digits and a
/// public key block, or it's refused.
fn checked_key<'a>(name: &str, key: Option<&'a RepoKey>) -> Result<Option<&'a RepoKey>, String> {
    match key {
        Some(k)
            if k.fingerprint.len() != 40
                || !k.fingerprint.bytes().all(|b| b.is_ascii_hexdigit())
                || !k
                    .armored
                    .starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----") =>
        {
            Err(format!("the key recorded for {name} doesn't look right"))
        }
        k => Ok(k),
    }
}

fn wrong(name: &str) -> String {
    format!("the settings recorded for {name} don't look right")
}

/// Omarchy on Arch: pacman, yay and the AUR, and Omarchy's own commands.
pub struct OmarchyArchAdapter;

impl PackageAdapter for OmarchyArchAdapter {
    fn installed(&self) -> BTreeSet<String> {
        lines(&run(None, "pacman", &["-Qq"]).unwrap_or_default()).collect()
    }

    fn explicit(&self) -> Vec<String> {
        lines(&run(None, "pacman", &["-Qqe"]).unwrap_or_default()).collect()
    }

    fn info(&self, names: &[&str]) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        if names.is_empty() {
            return Default::default();
        }
        let mut args = vec!["-Qi"];
        args.extend(names);
        let info = run(None, "pacman", &args).unwrap_or_default();
        (parse_info(&info, "Description"), parse_info(&info, "URL"))
    }

    /// pacman takes a package from the first repository in pacman.conf that has it (on
    /// Omarchy: omarchy, then Arch's core, extra and multilib), and yay turns to the AUR only
    /// when none does.
    fn origins(&self) -> BTreeMap<String, String> {
        pacman_origins(&run(None, "pacman", &["-Sl"]).unwrap_or_default())
    }

    fn unlisted_origin(&self) -> &'static str {
        "aur"
    }

    fn standard_repos(&self) -> &'static [&'static str] {
        &[
            "omarchy",
            "core",
            "extra",
            "multilib",
            "core-testing",
            "extra-testing",
            "multilib-testing",
        ]
    }

    fn repos_here(&self) -> BTreeSet<String> {
        lines(&run(None, "pacman-conf", &["--repo-list"]).unwrap_or_default()).collect()
    }

    /// Its servers and signature rules as pacman uses them, with the key that signs it.
    fn read_repository(&self, name: &str) -> Option<Repository> {
        let conf = run(None, "pacman-conf", &[&format!("--repo={name}")]).unwrap_or_default();
        let (servers, sig_level) = pacman_repository(&conf);
        Some(Repository {
            servers,
            sig_level,
            key: repo_key(name),
        })
    }

    /// Its settings file in /etc/pacman.d, an Include for it in pacman.conf, its key imported
    /// and trusted, and the package lists refreshed.
    fn repository_script(
        &self,
        name: &str,
        repo: &Repository,
        dir: &Path,
    ) -> Result<String, String> {
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
            return Err(wrong(name));
        }
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
            "install -m 644 '{}' '{target}'\ngrep -qxF '{include}' /etc/pacman.conf || printf '\\n%s\\n' '{include}' >> /etc/pacman.conf\n",
            dir.join("repo.conf").display()
        );
        std::fs::write(dir.join("repo.conf"), conf).map_err(|e| e.to_string())?;
        if let Some(key) = checked_key(name, repo.key.as_ref())? {
            std::fs::write(dir.join("repo.key"), &key.armored).map_err(|e| e.to_string())?;
            script.push_str(&format!(
                "pacman-key --add '{}'\npacman-key --lsign-key {}\n",
                dir.join("repo.key").display(),
                key.fingerprint
            ));
        }
        script.push_str("pacman -Sy --noconfirm\n");
        Ok(script)
    }

    fn install_command(&self, apps: &[String]) -> Vec<String> {
        [
            "yay",
            "-S",
            "--needed",
            "--noconfirm",
            "--sudo",
            "pkexec",
            "--",
        ]
        .into_iter()
        .map(String::from)
        .chain(apps.iter().cloned())
        .collect()
    }

    fn helpers_command(&self, need: &[&str]) -> Vec<String> {
        let mut args = vec!["-S", "--needed", "--noconfirm"];
        args.extend(
            need.iter()
                .map(|&t| if t == "gh" { "github-cli" } else { t }),
        );
        self.as_root("pacman", &args)
    }

    fn owner(&self, path: &str) -> Option<String> {
        pacman_owner(&run(None, "pacman", &["-Qo", path]).unwrap_or_default())
    }
}

impl DesktopAdapter for OmarchyArchAdapter {
    fn profile(&self) -> DistroProfile {
        DistroProfile::OMARCHY
    }

    fn desktop_version(&self) -> Option<String> {
        run(None, "omarchy", &["version"]).ok()
    }

    fn set_theme(&self, name: &str) -> Result<(), String> {
        run(None, "omarchy", &["theme", "set", name]).map(|_| ())
    }

    fn set_background(&self, image: &Path) -> Result<(), String> {
        let image = image.to_string_lossy();
        run(None, "omarchy", &["theme", "bg", "set", &image]).map(|_| ())
    }

    fn plugin(&self, args: &[&str]) -> Result<String, String> {
        let mut all = vec!["plugin"];
        all.extend_from_slice(args);
        run(None, "omarchy", &all)
    }

    /// Only whether the shell's command is there: it answers `--version` with its usage and
    /// exit code 1, so `has` would say it's missing on every Omarchy.
    fn has_shell(&self) -> bool {
        Command::new("omarchy-shell")
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .is_ok()
    }

    fn shell(&self, args: &[&str]) -> Result<String, String> {
        run(None, "omarchy-shell", args)
    }

    fn restart_shell(&self) -> Result<(), String> {
        run(None, "omarchy", &["restart", "shell"]).map(|_| ())
    }
}

impl PrivilegeAdapter for OmarchyArchAdapter {}

/// Package → source from `pacman -Sl`, which lists repositories in pacman.conf's order: the
/// first one listing a package wins.
fn pacman_origins(listing: &str) -> BTreeMap<String, String> {
    let mut origin = BTreeMap::new();
    for line in listing.lines() {
        let mut parts = line.split(' ');
        if let (Some(repo), Some(name)) = (parts.next(), parts.next()) {
            let source = match repo {
                "omarchy" => "omarchy",
                "core" | "extra" | "multilib" => "arch",
                repo => repo,
            };
            origin
                .entry(name.to_string())
                .or_insert_with(|| source.to_string());
        }
    }
    origin
}

/// Servers and signature rules from `pacman-conf --repo=name`.
fn pacman_repository(conf: &str) -> (Vec<String>, Vec<String>) {
    let field = |key: &str| -> Vec<String> {
        conf.lines()
            .filter_map(|l| l.strip_prefix(key))
            .map(|v| v.trim().to_string())
            .collect()
    };
    (field("Server = "), field("SigLevel = "))
}

/// The package in `pacman -Qo`: "/usr/bin/omarchy-sync is owned by omarchy-sync 0.1.1-1".
pub fn pacman_owner(out: &str) -> Option<String> {
    out.rsplit_once(" is owned by ")
        .and_then(|(_, s)| s.split_whitespace().next())
        .map(String::from)
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

/// A public key file, with the fingerprint and owner gpg reads from it.
fn armored_key(path: &Path) -> Option<RepoKey> {
    let armored = std::fs::read_to_string(path).ok()?;
    let tmp = TempDir::new("keys").ok()?;
    let home = tmp.path().to_string_lossy().into_owned();
    let path = path.to_string_lossy();
    let args = ["--homedir", &home, "--no-autostart", "--batch"];
    let shown = run(
        None,
        "gpg",
        &[&args[..], &["--with-colons", "--show-keys", &path]].concat(),
    )
    .ok()?;
    let (fingerprint, owner) = parse_colons(&shown)?;
    armored
        .starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----")
        .then_some(RepoKey {
            fingerprint,
            owner,
            armored,
        })
}

/// The first fingerprint and owner in gpg's `--with-colons` listing.
fn parse_colons(out: &str) -> Option<(String, String)> {
    let field = |kind: &str, n: usize| {
        out.lines()
            .map(|l| l.split(':').collect::<Vec<_>>())
            .find(|f| f[0] == kind)
            .and_then(|f| f.get(n).map(|s| s.to_string()))
    };
    let fingerprint = field("fpr", 9)?.to_uppercase();
    Some((fingerprint, field("uid", 9).unwrap_or_default()))
}

/// The file names in a folder that end in one of `ends`, without it.
fn stems(dir: &str, ends: &[&str]) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            ends.iter()
                .find_map(|end| name.strip_suffix(end))
                .map(String::from)
        })
        .collect()
}

/// Name, description and website from tab-separated lines (dpkg-query's and rpm's formats).
fn parse_tabbed(out: &str) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let (mut about, mut website) = (BTreeMap::new(), BTreeMap::new());
    for line in out.lines() {
        let mut parts = line.split('\t').map(str::trim);
        if let (Some(name), Some(summary)) = (parts.next(), parts.next())
            && !name.is_empty()
        {
            about.insert(name.to_string(), summary.to_string());
            if let Some(url) = parts.next().filter(|u| !u.is_empty()) {
                website.insert(name.to_string(), url.to_string());
            }
        }
    }
    (about, website)
}

/// Debian, Ubuntu and their relatives: dpkg and apt.
pub struct AptAdapter;

/// Where apt keeps the keys that sign extra repositories (`signed-by`).
const APT_KEYRINGS: &str = "/etc/apt/keyrings";

impl PackageAdapter for AptAdapter {
    fn installed(&self) -> BTreeSet<String> {
        let args = ["-W", "-f=${db:Status-Abbrev} ${Package}\n"];
        dpkg_installed(&run(None, "dpkg-query", &args).unwrap_or_default())
    }

    fn explicit(&self) -> Vec<String> {
        lines(&run(None, "apt-mark", &["showmanual"]).unwrap_or_default()).collect()
    }

    fn info(&self, names: &[&str]) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let mut args = vec![
            "-W",
            "-f=${Package}\t${Binary:Summary}\t${Homepage}\n",
            "--",
        ];
        args.extend(names);
        parse_tabbed(&run(None, "dpkg-query", &args).unwrap_or_default())
    }

    // shortcut: every package counts as the distribution's, so apps from an extra repository
    // aren't tied to it; read `apt-cache policy` when sharing those across computers matters.
    fn unlisted_origin(&self) -> &'static str {
        "distro"
    }

    fn standard_repos(&self) -> &'static [&'static str] {
        &["ubuntu", "debian", "ubuntu-esm-apps", "ubuntu-esm-infra"]
    }

    fn repos_here(&self) -> BTreeSet<String> {
        stems("/etc/apt/sources.list.d", &[".list", ".sources"])
    }

    // shortcut: only the one-line `.list` format; deb822 `.sources` files aren't recorded.
    fn read_repository(&self, name: &str) -> Option<Repository> {
        let list = std::fs::read_to_string(format!("/etc/apt/sources.list.d/{name}.list")).ok()?;
        Some(Repository {
            servers: apt_lines(&list),
            sig_level: Vec::new(),
            key: armored_key(&Path::new(APT_KEYRINGS).join(format!("{name}.asc"))),
        })
    }

    /// Its key in /etc/apt/keyrings, its `deb` lines in sources.list.d signed by that key, and
    /// the package lists refreshed.
    fn repository_script(
        &self,
        name: &str,
        repo: &Repository,
        dir: &Path,
    ) -> Result<String, String> {
        let key = checked_key(name, repo.key.as_ref())?;
        let keyring = format!("{APT_KEYRINGS}/{name}.asc");
        let list = apt_list(&repo.servers, key.map(|_| keyring.as_str())).ok_or(wrong(name))?;
        let mut script = String::new();
        if let Some(key) = key {
            std::fs::write(dir.join("repo.key"), &key.armored).map_err(|e| e.to_string())?;
            script.push_str(&format!(
                "install -d -m 755 {APT_KEYRINGS}\ninstall -m 644 '{}' '{keyring}'\n",
                dir.join("repo.key").display()
            ));
        }
        std::fs::write(dir.join("repo.list"), list).map_err(|e| e.to_string())?;
        script.push_str(&format!(
            "install -m 644 '{}' '/etc/apt/sources.list.d/{name}.list'\napt-get update\n",
            dir.join("repo.list").display()
        ));
        Ok(script)
    }

    fn install_command(&self, apps: &[String]) -> Vec<String> {
        // No questions from a package's setup: nobody is at a terminal to answer.
        let mut args = vec!["DEBIAN_FRONTEND=noninteractive", "apt-get", "install", "-y"];
        args.push("--");
        args.extend(apps.iter().map(String::as_str));
        self.as_root("env", &args)
    }

    fn helpers_command(&self, need: &[&str]) -> Vec<String> {
        let mut args = vec!["install", "-y"];
        args.extend_from_slice(need);
        self.as_root("apt-get", &args)
    }

    fn owner(&self, path: &str) -> Option<String> {
        dpkg_owner(&run(None, "dpkg-query", &["-S", path]).ok()?)
    }
}

impl DesktopAdapter for AptAdapter {
    fn profile(&self) -> DistroProfile {
        DistroProfile::PLAIN
    }
}

impl PrivilegeAdapter for AptAdapter {}

/// Installed packages from `dpkg-query -W -f='${db:Status-Abbrev} ${Package}\n'`: only "ii",
/// not ones removed with their settings left behind.
fn dpkg_installed(out: &str) -> BTreeSet<String> {
    out.lines()
        .filter_map(|l| l.strip_prefix("ii "))
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// The package in `dpkg-query -S`: "ripgrep: /usr/bin/rg", or "libc6:amd64: /lib/…".
fn dpkg_owner(out: &str) -> Option<String> {
    let (owner, _) = out.lines().next()?.split_once(": ")?;
    let name = owner.split(':').next()?.trim();
    (!name.is_empty() && !name.contains([',', ' '])).then(|| name.to_string())
}

/// The `deb` lines of a sources.list file.
fn apt_lines(list: &str) -> Vec<String> {
    lines(list).filter(|l| l.starts_with("deb ")).collect()
}

/// A sources.list file from recorded `deb` lines, signed by `keyring` when there's a key:
/// none when a line isn't plain `deb [options] https://… suite [components]`. Options from the
/// other computer (its architecture, its keyring path) are dropped.
fn apt_list(servers: &[String], keyring: Option<&str>) -> Option<String> {
    if servers.is_empty() {
        return None;
    }
    let mut list = String::new();
    for line in servers {
        let rest = line.strip_prefix("deb ")?.trim_start();
        let rest = match rest.strip_prefix('[') {
            Some(r) => r.split_once(']')?.1,
            None => rest,
        };
        let words: Vec<&str> = rest.split_whitespace().collect();
        let plain = words.iter().all(|w| {
            w.chars()
                .all(|c| c.is_ascii_graphic() && !"'\"`$;&|<>(){}[]\\".contains(c))
        });
        if words.len() < 2 || !words[0].starts_with("https://") || !plain {
            return None;
        }
        match keyring {
            Some(k) => list.push_str(&format!("deb [signed-by={k}] {}\n", words.join(" "))),
            None => list.push_str(&format!("deb {}\n", words.join(" "))),
        }
    }
    Some(list)
}

/// Fedora and its relatives: rpm and dnf.
pub struct DnfAdapter;

/// Where rpm keys for extra repositories go.
const RPM_KEYS: &str = "/etc/pki/rpm-gpg";

impl PackageAdapter for DnfAdapter {
    fn installed(&self) -> BTreeSet<String> {
        lines(&run(None, "rpm", &["-qa", "--qf", "%{NAME}\n"]).unwrap_or_default()).collect()
    }

    fn explicit(&self) -> Vec<String> {
        let args = ["repoquery", "--userinstalled", "--qf", "%{name}\n"];
        lines(&run(None, "dnf", &args).unwrap_or_default()).collect()
    }

    fn info(&self, names: &[&str]) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let mut args = vec!["-q", "--qf", "%{NAME}\t%{SUMMARY}\t%{URL}\n", "--"];
        args.extend(names);
        parse_tabbed(&run(None, "rpm", &args).unwrap_or_default())
    }

    // shortcut: as for apt; `dnf repoquery --installed --qf '%{from_repo}'` would tell.
    fn unlisted_origin(&self) -> &'static str {
        "distro"
    }

    fn standard_repos(&self) -> &'static [&'static str] {
        &[
            "fedora",
            "fedora-updates",
            "fedora-updates-testing",
            "fedora-updates-archive",
            "fedora-cisco-openh264",
        ]
    }

    fn repos_here(&self) -> BTreeSet<String> {
        stems("/etc/yum.repos.d", &[".repo"])
    }

    // shortcut: the file's first section only (not its -debuginfo or -source ones).
    fn read_repository(&self, name: &str) -> Option<Repository> {
        let text = std::fs::read_to_string(format!("/etc/yum.repos.d/{name}.repo")).ok()?;
        let (servers, key_path) = dnf_section(&text);
        Some(Repository {
            servers,
            sig_level: Vec::new(),
            key: key_path.and_then(|p| armored_key(Path::new(&p))),
        })
    }

    /// Its key imported into rpm, its settings in /etc/yum.repos.d, and the package lists
    /// refreshed.
    fn repository_script(
        &self,
        name: &str,
        repo: &Repository,
        dir: &Path,
    ) -> Result<String, String> {
        let key = checked_key(name, repo.key.as_ref())?;
        let target = format!("{RPM_KEYS}/{name}.asc");
        let conf =
            dnf_repo(name, &repo.servers, key.map(|_| target.as_str())).ok_or(wrong(name))?;
        let mut script = String::new();
        if let Some(key) = key {
            std::fs::write(dir.join("repo.key"), &key.armored).map_err(|e| e.to_string())?;
            script.push_str(&format!(
                "install -m 644 '{}' '{target}'\nrpm --import '{target}'\n",
                dir.join("repo.key").display()
            ));
        }
        std::fs::write(dir.join("repo.conf"), conf).map_err(|e| e.to_string())?;
        script.push_str(&format!(
            "install -m 644 '{}' '/etc/yum.repos.d/{name}.repo'\ndnf makecache\n",
            dir.join("repo.conf").display()
        ));
        Ok(script)
    }

    fn install_command(&self, apps: &[String]) -> Vec<String> {
        let mut args = vec!["install", "-y", "--"];
        args.extend(apps.iter().map(String::as_str));
        self.as_root("dnf", &args)
    }

    fn helpers_command(&self, need: &[&str]) -> Vec<String> {
        let mut args = vec!["install", "-y"];
        args.extend_from_slice(need);
        self.as_root("dnf", &args)
    }

    fn owner(&self, path: &str) -> Option<String> {
        let out = run(None, "rpm", &["-qf", "--qf", "%{NAME}\n", path]).ok()?;
        lines(&out).next()
    }
}

impl DesktopAdapter for DnfAdapter {
    fn profile(&self) -> DistroProfile {
        DistroProfile::PLAIN
    }
}

impl PrivilegeAdapter for DnfAdapter {}

/// The settings a recorded dnf repository may carry; its key is written by this computer.
const DNF_KEYS: &[&str] = &[
    "name",
    "baseurl",
    "metalink",
    "mirrorlist",
    "enabled",
    "gpgcheck",
    "repo_gpgcheck",
    "type",
    "skip_if_unavailable",
];

/// A .repo file's first section: the settings worth recording (`key=value` lines), and the
/// local file its key is in.
fn dnf_section(text: &str) -> (Vec<String>, Option<String>) {
    let (mut settings, mut key) = (Vec::new(), None);
    let mut sections = 0;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            sections += 1;
        } else if sections == 1
            && let Some((k, v)) = line.split_once('=')
        {
            let (k, v) = (k.trim(), v.trim());
            if k == "gpgkey" {
                key = v.strip_prefix("file://").map(String::from);
            } else if DNF_KEYS.contains(&k) {
                settings.push(format!("{k}={v}"));
            }
        }
    }
    (settings, key)
}

/// A .repo file from recorded settings, with its key at `key` when there is one: none when a
/// setting isn't one dnf_section records, or an address isn't https.
fn dnf_repo(name: &str, settings: &[String], key: Option<&str>) -> Option<String> {
    let mut conf = format!("[{name}]\n");
    let mut addresses = 0;
    for line in settings {
        let (k, v) = line.split_once('=')?;
        let (k, v) = (k.trim(), v.trim());
        let address = matches!(k, "baseurl" | "metalink" | "mirrorlist");
        let ok = DNF_KEYS.contains(&k)
            && !v.is_empty()
            && !v.chars().any(|c| c.is_control())
            && (!address || (v.starts_with("https://") && !v.contains(char::is_whitespace)));
        if !ok {
            return None;
        }
        addresses += usize::from(address);
        conf.push_str(&format!("{k}={v}\n"));
    }
    if addresses == 0 {
        return None;
    }
    if let Some(k) = key {
        conf.push_str(&format!("gpgkey=file://{k}\n"));
    }
    Some(conf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARCH: &str = "NAME=\"Arch Linux\"\nID=arch\nBUILD_ID=rolling\n";
    const UBUNTU: &str = "NAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\nID=ubuntu\nID_LIKE=debian\n";
    const DEBIAN: &str = "PRETTY_NAME=\"Debian GNU/Linux 12\"\nID=debian\n";
    const MINT: &str = "ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n";
    const FEDORA: &str = "NAME=\"Fedora Linux\"\nID=fedora\nVERSION_ID=41\n";
    const ROCKY: &str = "ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n";
    const VOID: &str = "NAME=\"Void\"\nID=\"void\"\n";

    #[test]
    fn os_release_names_the_family() {
        assert_eq!(parse_os_release(ARCH), Some(DistroKind::Arch));
        assert_eq!(
            parse_os_release("ID=manjaro\nID_LIKE=arch\n"),
            Some(DistroKind::Arch)
        );
        assert_eq!(parse_os_release(UBUNTU), Some(DistroKind::Debian));
        assert_eq!(parse_os_release(DEBIAN), Some(DistroKind::Debian));
        assert_eq!(parse_os_release(MINT), Some(DistroKind::Debian));
        assert_eq!(parse_os_release(FEDORA), Some(DistroKind::Fedora));
        assert_eq!(parse_os_release(ROCKY), Some(DistroKind::Fedora));
        assert_eq!(parse_os_release(VOID), None);
        assert_eq!(parse_os_release(""), None);
        // VERSION_ID isn't ID.
        assert_eq!(parse_os_release("VERSION_ID=debian\n"), None);
    }

    #[test]
    fn omarchy_and_arch_get_everything_others_only_their_apps() {
        let apps = ["ripgrep".to_string()];
        for (text, omarchy) in [(ARCH, false), (UBUNTU, true), (FEDORA, true), (VOID, false)] {
            let p = detect(text, omarchy);
            assert_eq!(p.profile(), DistroProfile::OMARCHY, "{text}");
            assert_eq!(p.install_command(&apps)[0], "yay");
        }
        let apt = detect(UBUNTU, false);
        assert_eq!(apt.profile(), DistroProfile::PLAIN);
        assert_eq!(
            apt.install_command(&apps),
            [
                "pkexec",
                "env",
                "DEBIAN_FRONTEND=noninteractive",
                "apt-get",
                "install",
                "-y",
                "--",
                "ripgrep"
            ]
        );
        let dnf = detect(FEDORA, false);
        assert_eq!(dnf.profile(), DistroProfile::PLAIN);
        assert_eq!(
            dnf.install_command(&apps),
            ["pkexec", "dnf", "install", "-y", "--", "ripgrep"]
        );
        assert_eq!(
            OmarchyArchAdapter.install_command(&apps),
            [
                "yay",
                "-S",
                "--needed",
                "--noconfirm",
                "--sudo",
                "pkexec",
                "--",
                "ripgrep"
            ]
        );
    }

    #[test]
    fn profiles_say_what_the_desktop_can_do() {
        let o = DistroProfile::OMARCHY;
        assert!(
            o.supports_themes
                && o.supports_bar_indicator
                && o.supports_plugins
                && o.supports_major_version_guard
        );
        let p = DistroProfile::PLAIN;
        assert!(
            !p.supports_themes
                && !p.supports_bar_indicator
                && !p.supports_plugins
                && !p.supports_major_version_guard
        );
        // A plain desktop has none of Omarchy's commands, and says so instead of running one.
        assert_eq!(AptAdapter.desktop_version(), None);
        assert!(!DnfAdapter.has_shell());
        assert!(AptAdapter.set_theme("x").is_err());
        assert!(AptAdapter.set_background(Path::new("/x")).is_err());
        assert!(DnfAdapter.plugin(&["list"]).is_err());
        assert!(DnfAdapter.shell(&["shell"]).is_err());
        assert!(AptAdapter.restart_shell().is_ok());
    }

    #[test]
    fn setup_tools_install_under_each_distributions_names() {
        let need = ["chezmoi", "gh", "git"];
        assert_eq!(
            OmarchyArchAdapter.helpers_command(&need),
            [
                "pkexec",
                "pacman",
                "-S",
                "--needed",
                "--noconfirm",
                "chezmoi",
                "github-cli",
                "git"
            ]
        );
        assert_eq!(
            AptAdapter.helpers_command(&need),
            ["pkexec", "apt-get", "install", "-y", "chezmoi", "gh", "git"]
        );
        assert_eq!(
            DnfAdapter.helpers_command(&need),
            ["pkexec", "dnf", "install", "-y", "chezmoi", "gh", "git"]
        );
    }

    #[test]
    fn root_commands_go_through_the_password_window() {
        assert_eq!(
            OmarchyArchAdapter.as_root("pacman", &["-S", "git"]),
            ["pkexec", "pacman", "-S", "git"]
        );
        assert_eq!(AptAdapter.as_root("true", &[]), ["pkexec", "true"]);
        assert_eq!(DnfAdapter.terminal_root(), "sudo ");
    }

    #[test]
    fn reads_package_lists_and_owners() {
        let dpkg = "ii ripgrep\nrc oldthing\nii  \nii libc6\n";
        assert_eq!(
            dpkg_installed(dpkg),
            BTreeSet::from(["ripgrep".to_string(), "libc6".to_string()])
        );
        assert_eq!(
            lines("firefox\n\n  git \n").collect::<Vec<_>>(),
            ["firefox", "git"]
        );
        assert_eq!(
            dpkg_owner("ripgrep: /usr/bin/rg").as_deref(),
            Some("ripgrep")
        );
        assert_eq!(
            dpkg_owner("libc6:amd64: /lib/x86_64-linux-gnu/libc.so.6").as_deref(),
            Some("libc6")
        );
        assert_eq!(dpkg_owner("diversion by x from: /usr/bin/y"), None);
        assert_eq!(
            pacman_owner("/usr/bin/omarchy-sync is owned by omarchy-sync 0.1.1-1").as_deref(),
            Some("omarchy-sync")
        );
        assert_eq!(
            pacman_owner("error: No package owns /home/me/.local/bin/omarchy-sync"),
            None
        );
        let origins = pacman_origins(
            "omarchy walker 1\ncore bash 5\nextra firefox 1\nextra walker 0\nchaotic-aur zen 1\n",
        );
        assert_eq!(origins["walker"], "omarchy");
        assert_eq!(origins["bash"], "arch");
        assert_eq!(origins["firefox"], "arch");
        assert_eq!(origins["zen"], "chaotic-aur");
    }

    #[test]
    fn reads_package_descriptions() {
        let (about, website) = parse_tabbed(
            "ripgrep\tFast grep\thttps://github.com/BurntSushi/ripgrep\nfoo\tA foo\t\n",
        );
        assert_eq!(about["ripgrep"], "Fast grep");
        assert_eq!(about["foo"], "A foo");
        assert_eq!(website["ripgrep"], "https://github.com/BurntSushi/ripgrep");
        assert!(!website.contains_key("foo"));
        assert_eq!(
            pacman_repository("Server = https://a/x\nSigLevel = Required\nServer = https://b/x\n"),
            (
                vec!["https://a/x".to_string(), "https://b/x".to_string()],
                vec!["Required".to_string()]
            )
        );
        let colons = "pub:-:4096:1:ABCDEF:1:::-:::scESC::::::23::0:\nfpr:::::::::0123456789abcdef0123456789abcdef01234567:\nuid:-::::1::HASH::Example Repo <repo@example.com>::::::::::0:\n";
        assert_eq!(
            parse_colons(colons),
            Some((
                "0123456789ABCDEF0123456789ABCDEF01234567".to_string(),
                "Example Repo <repo@example.com>".to_string()
            ))
        );
    }

    fn key() -> RepoKey {
        RepoKey {
            fingerprint: "0123456789ABCDEF0123456789ABCDEF01234567".into(),
            owner: "Example".into(),
            armored: "-----BEGIN PGP PUBLIC KEY BLOCK-----\nx\n".into(),
        }
    }

    #[test]
    fn apt_repositories_are_signed_by_their_key_and_checked() {
        let list = "# a comment\ndeb [arch=amd64 signed-by=/usr/share/keyrings/x.gpg] https://packages.example.com/apt stable main\ndeb-src https://packages.example.com/apt stable main\n";
        let servers = apt_lines(list);
        assert_eq!(servers.len(), 1);
        assert_eq!(
            apt_list(&servers, Some("/etc/apt/keyrings/example.asc")).as_deref(),
            Some(
                "deb [signed-by=/etc/apt/keyrings/example.asc] https://packages.example.com/apt stable main\n"
            )
        );
        for bad in [
            "deb http://plain.example.com stable main",
            "deb https://x.example.com",
            "deb https://x.example.com stable main; rm -rf /",
            "deb https://x.example.com $(id) main",
            "https://x.example.com",
            "Server = https://x.example.com",
        ] {
            assert_eq!(apt_list(&[bad.to_string()], None), None, "{bad}");
        }
        assert_eq!(apt_list(&[], None), None);

        let tmp = TempDir::new("apt-test").unwrap();
        let repo = Repository {
            servers,
            sig_level: Vec::new(),
            key: Some(key()),
        };
        let script = AptAdapter
            .repository_script("example", &repo, tmp.path())
            .unwrap();
        assert!(script.contains("'/etc/apt/keyrings/example.asc'"));
        assert!(script.contains("'/etc/apt/sources.list.d/example.list'"));
        assert!(script.ends_with("apt-get update\n"));
        assert!(tmp.path().join("repo.key").exists());
        let bad_key = Repository {
            key: Some(RepoKey {
                fingerprint: "nope".into(),
                ..key()
            }),
            ..repo.clone()
        };
        assert!(
            AptAdapter
                .repository_script("example", &bad_key, tmp.path())
                .is_err()
        );
        // An Arch computer's repository isn't added here, nor this one there.
        let arch = Repository {
            servers: vec!["https://example.com/$arch".into()],
            sig_level: vec!["Required".into()],
            key: None,
        };
        assert!(
            AptAdapter
                .repository_script("x", &arch, tmp.path())
                .is_err()
        );
        assert!(
            OmarchyArchAdapter
                .repository_script("x", &repo, tmp.path())
                .is_err()
        );
    }

    #[test]
    fn dnf_repositories_keep_only_known_settings() {
        let text = "[example]\nname=Example $releasever\nbaseurl=https://rpm.example.com/$basearch\nenabled=1\ngpgcheck=1\ngpgkey=file:///etc/pki/rpm-gpg/example.asc\nexclude=foo\n\n[example-source]\nbaseurl=https://rpm.example.com/source\n";
        let (settings, key_path) = dnf_section(text);
        assert_eq!(
            settings,
            [
                "name=Example $releasever",
                "baseurl=https://rpm.example.com/$basearch",
                "enabled=1",
                "gpgcheck=1"
            ]
        );
        assert_eq!(key_path.as_deref(), Some("/etc/pki/rpm-gpg/example.asc"));
        assert_eq!(
            dnf_repo("example", &settings, Some("/etc/pki/rpm-gpg/example.asc")).as_deref(),
            Some(
                "[example]\nname=Example $releasever\nbaseurl=https://rpm.example.com/$basearch\nenabled=1\ngpgcheck=1\ngpgkey=file:///etc/pki/rpm-gpg/example.asc\n"
            )
        );
        for bad in [
            vec!["baseurl=http://plain.example.com".to_string()],
            vec!["name=only a name".to_string()],
            vec!["sslclientkey=/root/key".to_string()],
            vec!["https://example.com/arch".to_string()],
            vec!["deb https://x.example.com stable main".to_string()],
        ] {
            assert_eq!(dnf_repo("x", &bad, None), None, "{bad:?}");
        }

        let tmp = TempDir::new("dnf-test").unwrap();
        let repo = Repository {
            servers: settings,
            sig_level: Vec::new(),
            key: Some(key()),
        };
        let script = DnfAdapter
            .repository_script("example", &repo, tmp.path())
            .unwrap();
        assert!(script.contains("rpm --import '/etc/pki/rpm-gpg/example.asc'"));
        assert!(script.contains("'/etc/yum.repos.d/example.repo'"));
        assert!(script.ends_with("dnf makecache\n"));
    }

    #[test]
    fn arch_repository_script_is_unchanged() {
        let tmp = TempDir::new("arch-test").unwrap();
        let repo = Repository {
            servers: vec!["https://example.com/arch".into()],
            sig_level: vec!["Required".into()],
            key: Some(key()),
        };
        let dir = tmp.path();
        let script = OmarchyArchAdapter
            .repository_script("example", &repo, dir)
            .unwrap();
        assert_eq!(
            script,
            format!(
                "install -m 644 '{0}/repo.conf' '/etc/pacman.d/omarchy-sync-example.conf'\ngrep -qxF 'Include = /etc/pacman.d/omarchy-sync-example.conf' /etc/pacman.conf || printf '\\n%s\\n' 'Include = /etc/pacman.d/omarchy-sync-example.conf' >> /etc/pacman.conf\npacman-key --add '{0}/repo.key'\npacman-key --lsign-key 0123456789ABCDEF0123456789ABCDEF01234567\npacman -Sy --noconfirm\n",
                dir.display()
            )
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("repo.conf")).unwrap(),
            "[example]\nSigLevel = Required\nServer = https://example.com/arch\n"
        );
    }
}
