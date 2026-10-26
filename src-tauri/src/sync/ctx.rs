//! Where things are (`Ctx`), running tools, git progress, the sync lock, and the sentences errors become.

use super::*;

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
        // OMARCHY_SYNC_HOST stands in for the machine ID where several "computers" share one
        // machine (the end-to-end tests); the ignore rules read it too. Nothing else sets it.
        let id = std::env::var("OMARCHY_SYNC_HOST")
            .or_else(|_| std::fs::read_to_string("/etc/machine-id"))
            .unwrap_or_default();
        let id: String = id.trim().chars().take(12).collect();
        Ctx {
            // shortcut: assumes chezmoi's default source folder, upgrade to `chezmoi source-path` if anyone sets sourceDir.
            src: data_home.join("chezmoi"),
            host: if id.is_empty() { hostname.clone() } else { id },
            hostname,
            home,
        }
    }

    pub(crate) fn data_path(&self) -> PathBuf {
        self.src.join(".chezmoidata.yaml")
    }

    /// Whether the sync choices differ from the last saved sync. Order doesn't count, so a
    /// switch flipped and flipped back is no change.
    /// What changed in the choices since they were last shared, in sentences; none when nothing.
    pub fn choice_changes(&self) -> Vec<String> {
        let read = |text: &str| {
            serde_yaml::from_str::<Data>(text).ok().map(|mut d| {
                d.upgrade();
                d.sorted()
            })
        };
        let saved = self.git(&["show", "HEAD:.chezmoidata.yaml"]).ok();
        let now = std::fs::read_to_string(self.data_path()).ok();
        match (
            saved.as_deref().and_then(read),
            now.as_deref().and_then(read),
        ) {
            (Some(before), Some(after)) => {
                describe_changes(&before, &after, &self.host, |f| label(f).to_string())
            }
            (None, None) => Vec::new(),
            _ => vec!["Other sync choices changed".to_string()],
        }
    }

    /// Plugin folders on this computer.
    /// Web app shortcuts here (they open omarchy-launch-webapp) and the icon files they use.
    pub fn local_web_apps(&self) -> (Vec<String>, Vec<String>) {
        let names = |dir: &str| -> Vec<String> {
            std::fs::read_dir(self.home.join(dir))
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| is_file_name(n))
                .collect()
        };
        let icon_files = names(ICON_DIR);
        let (mut apps, mut icons) = (Vec::new(), Vec::new());
        for name in names(APPS_DIR)
            .into_iter()
            .filter(|n| n.ends_with(".desktop"))
        {
            let text =
                std::fs::read_to_string(self.home.join(APPS_DIR).join(&name)).unwrap_or_default();
            let web = text.lines().any(|l| {
                l.starts_with("Exec=")
                    && (l.contains("omarchy-launch-webapp") || l.contains("--app="))
            });
            if !web {
                continue;
            }
            if let Some(icon) = text.lines().find_map(|l| l.strip_prefix("Icon=")) {
                icons.extend(
                    icon_files
                        .iter()
                        .filter(|f| {
                            f.rsplit_once('.')
                                .is_some_and(|(stem, _)| stem == icon.trim())
                        })
                        .cloned(),
                );
            }
            apps.push(name);
        }
        apps.sort();
        icons.sort();
        icons.dedup();
        (apps, icons)
    }

    pub fn local_plugins(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(self.home.join(PLUGIN_DIR))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|id| is_plugin_id(id))
            .collect();
        ids.sort();
        ids
    }

    /// Names a plugin from its manifest, here or (for one this computer doesn't have) in the
    /// sync folder.
    pub fn plugin(&self, id: &str) -> Plugin {
        let dir = self.home.join(PLUGIN_DIR).join(id);
        let manifest = [&dir, &self.src.join("dot_config/omarchy/plugins").join(id)]
            .iter()
            .find_map(|dir| std::fs::read(dir.join("manifest.json")).ok())
            .and_then(|m| serde_json::from_slice::<serde_json::Value>(&m).ok())
            .unwrap_or_default();
        let text = |k: &str| manifest[k].as_str().unwrap_or_default().trim().to_string();
        let name = text("name");
        Plugin {
            id: id.to_string(),
            name: if name.is_empty() {
                id.to_string()
            } else {
                name
            },
            description: text("description"),
            kinds: manifest["kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|k| k.as_str().map(String::from))
                .collect(),
            here: dir.is_dir(),
            source: self.plugin_source(id),
            ..Default::default()
        }
    }

    /// The git address a plugin here was added from, if it was.
    pub fn plugin_source(&self, id: &str) -> Option<String> {
        let dir = self.home.join(PLUGIN_DIR).join(id);
        if !dir.join(".git").exists() {
            return None;
        }
        run(Some(&dir), "git", &["remote", "get-url", "origin"])
            .ok()
            .filter(|url| is_git_address(url))
    }

    pub fn load_data(&self) -> Data {
        // A file that doesn't read (a typo, a clash's markers) isn't "no choices", which would
        // stop syncing every folder: the last saved choices stand in until it reads again.
        let mut data: Data = std::fs::read_to_string(self.data_path())
            .ok()
            .and_then(|s| serde_yaml::from_str(&s).ok())
            .or_else(|| {
                self.git(&["show", "HEAD:.chezmoidata.yaml"])
                    .ok()
                    .and_then(|s| serde_yaml::from_str(&s).ok())
            })
            .unwrap_or_default();
        data.adopt_legacy(&self.host, &self.hostname);
        if data.version < 2 {
            // So this computer's plugins count among those that synced before.
            data.computers.entry(self.host.clone()).or_default().plugins = self.local_plugins();
        }
        data.upgrade();
        data
    }

    /// Written whole and then renamed into place, so a sync that reads it at the same moment
    /// (the background timer, say) never sees half a file.
    pub fn save_data(&self, data: &Data) -> Result<(), String> {
        let yaml = serde_yaml::to_string(data).map_err(|e| e.to_string())?;
        // A dot name: chezmoi doesn't look at those, and it's gone before git looks.
        let tmp = self.src.join(".omarchy-sync-data.tmp");
        std::fs::write(&tmp, yaml).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.data_path()).map_err(|e| e.to_string())
    }

    /// Fails when the choices file is there but can't be read (a typo in a hand edit, a field
    /// a newer version changed). `load_data` then reads the last saved choices, but saving those
    /// over it would lose the edit, so nothing saves until it reads again.
    pub fn data_readable(&self) -> Result<(), String> {
        match std::fs::read_to_string(self.data_path()) {
            Ok(text) if serde_yaml::from_str::<Data>(&text).is_err() => {
                Err("the sync choices file can't be read".into())
            }
            _ => Ok(()),
        }
    }

    pub(crate) fn git(&self, args: &[&str]) -> Result<String, String> {
        run(Some(&self.src), "git", args)
    }

    pub(crate) fn merging(&self) -> bool {
        self.src.join(".git/MERGE_HEAD").exists()
    }

    pub(crate) fn rebasing(&self) -> bool {
        let git_dir = self.src.join(".git");
        git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
    }

    pub(crate) fn conflicted(&self) -> Vec<String> {
        self.git(&["diff", "--name-only", "--diff-filter=U"])
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    /// Where Omarchy keeps what's current (theme, background) and the app its status.
    pub fn state_dir(&self) -> PathBuf {
        std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or(self.home.join(".local/state"))
    }

    fn theme_file(&self) -> PathBuf {
        self.state_dir().join("omarchy/current/theme.name")
    }

    /// The image the desktop shows now (Omarchy links it from its state folder).
    pub fn current_background(&self) -> Option<PathBuf> {
        let link = self.theme_file().with_file_name("background");
        std::fs::canonicalize(link).ok().filter(|p| p.is_file())
    }

    pub fn current_theme(&self) -> Option<String> {
        std::fs::read_to_string(self.theme_file())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

/// A git address a plugin can be added from: https or ssh, with nothing a shell could read
/// into it. Checked before anything from the page or the shared file is passed to a command.
pub fn is_git_address(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("git@"))
        && url.len() <= 200
        && !url.contains("..")
        && !url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "'\"`$;&|<>(){}".contains(c))
}

pub fn run(dir: Option<&Path>, prog: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(prog);
    // English messages to read, in UTF-8: under plain "C", Qt-based tools (omarchy plugin)
    // complain and fail, and file names with non-English letters come out escaped.
    cmd.args(args).stdin(Stdio::null()).env("LC_ALL", "C.UTF-8");
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

// Besides plain step names, a job's `step` callback carries two kinds of line, read by the
// page (state.ts): the steps it's about to take, and how far along the current one is.
const PLAN: &str = "\u{1}plan\t";

const PERCENT: &str = "\u{1}percent\t";

/// Announces every step a job will take, so the page can show what's still to come.
pub(crate) fn plan(step: &dyn Fn(&str), steps: &[&str]) {
    step(&format!("{PLAN}{}", steps.join("\t")));
}

/// The percentage in a git progress line, for the transfer itself only (git also counts and
/// compresses objects, each from 0 to 100%).
fn transfer_percent(line: &str) -> Option<u8> {
    if !line.contains("Receiving objects") && !line.contains("Writing objects") {
        return None;
    }
    let before = &line[..line.find('%')?];
    let digits = before.trim_end().rsplit(' ').next()?;
    digits.parse().ok()
}

/// The percentage in a line of curl's progress bar (`-#`): "#####   23.4%".
pub(crate) fn curl_percent(line: &str) -> Option<u8> {
    let before = &line[..line.rfind('%')?];
    before.rsplit(' ').next()?.split('.').next()?.parse().ok()
}

/// A git command that talks to GitHub, passing its upload or download percentage on as it goes.
pub(crate) fn git_progress(
    ctx: &Ctx,
    args: &[&str],
    step: &dyn Fn(&str),
) -> Result<String, String> {
    run_progress(Some(&ctx.src), "git", args, transfer_percent, step)
}

/// Like `run`, for a command that prints its progress: `percent` reads a line of it, and each
/// new value goes to the page as the current step's percentage.
pub(crate) fn run_progress(
    dir: Option<&Path>,
    prog: &str,
    args: &[&str],
    percent: fn(&str) -> Option<u8>,
    step: &dyn Fn(&str),
) -> Result<String, String> {
    use std::io::Read;
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .env("LC_ALL", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let mut child = cmd.spawn().map_err(|e| format!("{prog}: {e}"))?;
    let mut stderr = child.stderr.take().ok_or(format!("{prog}: no output"))?;
    let (mut all, mut line, mut last) = (Vec::new(), Vec::new(), None);
    let mut buf = [0u8; 1024];
    // Progress lines end in \r as they update; each finished one in \n.
    while let Ok(n) = stderr.read(&mut buf)
        && n > 0
    {
        for &b in &buf[..n] {
            all.push(b);
            if b == b'\r' || b == b'\n' {
                let p = percent(&String::from_utf8_lossy(&line));
                if p.is_some() && p != last {
                    step(&format!("{PERCENT}{}", p.unwrap_or_default()));
                    last = p;
                }
                line.clear();
            } else {
                line.push(b);
            }
        }
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("{prog}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        // Only what follows the last progress update: the error itself, not rows of bars.
        let err = String::from_utf8_lossy(&all);
        let err = err.rsplit('\r').next().unwrap_or(&err);
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
    } else if e.contains("choices file can't be read") {
        "The file that holds your sync choices can't be read, so nothing was synced. Undoing the last change to it in History fixes it.".into()
    } else if e.contains("can't undo") {
        "This change can't be undone on its own, because later changes build on it.".into()
    } else if e.contains("rejected") || e.contains("non-fast-forward") {
        "Another computer synced at the same moment. Press Sync now again.".into()
    } else if e.contains("isn't the latest release") {
        "A newer release appeared in the meantime. Check for updates again.".into()
    } else if e.contains("gh auth login") {
        "Not signed in to GitHub. Run gh auth login in a terminal, then try again.".into()
    } else if e.contains("checksum") || e.contains("attestation") {
        "The downloaded update couldn't be verified, so nothing was installed.".into()
    } else {
        "Something went wrong. Nothing on this computer was lost.".into()
    }
}

/// Kept outside the sync folder, so it works before `chezmoi init` makes that folder and while
/// `disconnect` moves it aside.
pub(crate) fn lock_path(ctx: &Ctx) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let dir = ctx.state_dir().join("omarchy-sync");
    private_dir(&dir, true).map_err(|e| e.to_string())?;
    // An older version may have made it open to read.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| e.to_string())?;
    Ok(dir.join("sync.lock"))
}

/// Only one sync at a time, whether it comes from the window or the background timer.
pub(crate) fn lock(ctx: &Ctx) -> Result<std::fs::File, String> {
    let file = std::fs::File::create(lock_path(ctx)?).map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|_| "another sync is running".to_string())?;
    Ok(file)
}

/// Waits for any running sync (window or background timer) to finish, then holds the lock.
pub(crate) fn lock_wait(ctx: &Ctx) -> Result<std::fs::File, String> {
    let file = std::fs::File::create(lock_path(ctx)?).map_err(|e| e.to_string())?;
    file.lock().map_err(|e| e.to_string())?;
    Ok(file)
}

/// Loads the shared choices, applies one change and saves them, once any running sync (the
/// background timer's, say) is done: a switch flipped during a sync would otherwise be saved
/// over by it, then committed and pushed as the old value.
pub fn edit(ctx: &Ctx, change: impl FnOnce(&mut Data, &str)) -> Result<Data, String> {
    let _lock = lock_wait(ctx)?;
    edit_locked(ctx, change)
}

/// `edit` for a caller already holding the lock.
pub fn edit_locked(ctx: &Ctx, change: impl FnOnce(&mut Data, &str)) -> Result<Data, String> {
    ctx.data_readable()?;
    let mut data = ctx.load_data();
    change(&mut data, &ctx.host);
    ctx.save_data(&data)?;
    Ok(data)
}

/// Makes a folder only this user can open (0700). `recursive` makes missing parents too.
fn private_dir(dir: &Path, recursive: bool) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(recursive)
        .mode(0o700)
        .create(dir)
}

/// A fresh private folder for files handed to another program (pkexec, luac, gpg), removed
/// when dropped. Its name can't be guessed, and it's made new, never reused: a fixed name in
/// /tmp could be made first by another user, who could then swap what root installs.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new(prefix: &str) -> Result<TempDir, String> {
        // The first that's set: the user's runtime folder (private, cleared at logout), cache, /tmp.
        let var = |k: &str| {
            std::env::var_os(k)
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
        };
        let root = var("XDG_RUNTIME_DIR")
            .filter(|p| p.is_dir())
            .or_else(|| var("XDG_CACHE_HOME"))
            .or_else(|| var("HOME").map(|h| h.join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        TempDir::new_in(&root.join("omarchy-sync-tmp"), prefix)
    }

    pub(crate) fn new_in(base: &Path, prefix: &str) -> Result<TempDir, String> {
        use std::io::Read;
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let err = |e: std::io::Error| format!("temporary folder: {e}");
        match private_dir(base, true) {
            Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => return Err(err(e)),
            _ => {}
        }
        // Only a real folder of ours: in /tmp another user could have made it first.
        let meta = std::fs::symlink_metadata(base).map_err(err)?;
        let me = std::fs::metadata("/proc/self").map_err(err)?.uid();
        if !meta.is_dir() || meta.uid() != me {
            return Err(format!("temporary folder: {} isn't ours", base.display()));
        }
        if meta.mode() & 0o777 != 0o700 {
            std::fs::set_permissions(base, std::fs::Permissions::from_mode(0o700)).map_err(err)?;
        }
        let mut urandom = std::fs::File::open("/dev/urandom").map_err(err)?;
        for _ in 0..100 {
            let mut random = [0u8; 8];
            urandom.read_exact(&mut random).map_err(err)?;
            let name = format!(
                "{prefix}-{}-{}-{:016x}",
                std::process::id(),
                COUNT.fetch_add(1, Ordering::Relaxed),
                u64::from_ne_bytes(random)
            );
            let dir = base.join(name);
            // Not recursive: fails when the name exists, even as a symlink, so nothing is reused.
            match private_dir(&dir, false) {
                Ok(()) => return Ok(TempDir(dir)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(err(e)),
            }
        }
        Err("temporary folder: no free name".into())
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn test_base(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("omarchy-sync-test-{name}-{}", now()));
        let _ = std::fs::remove_dir_all(&base);
        base
    }

    #[test]
    fn temp_dirs_are_new_private_and_gone_when_dropped() {
        let base = test_base("tempdir");
        let dirs: Vec<TempDir> = std::thread::scope(|s| {
            let workers: Vec<_> = (0..8)
                .map(|_| {
                    s.spawn(|| {
                        (0..20)
                            .map(|_| TempDir::new_in(&base, "t").unwrap())
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            workers
                .into_iter()
                .flat_map(|w| w.join().unwrap())
                .collect()
        });
        let names: BTreeSet<&Path> = dirs.iter().map(|d| d.path()).collect();
        assert_eq!(names.len(), 160);
        for d in &dirs {
            let mode = std::fs::metadata(d.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        let base_mode = std::fs::metadata(&base).unwrap().permissions().mode();
        assert_eq!(base_mode & 0o777, 0o700);
        let kept = dirs[0].path().to_path_buf();
        std::fs::write(kept.join("file"), "x").unwrap();
        drop(dirs);
        assert!(!kept.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn temp_dirs_arent_named_by_process_alone_or_made_through_a_link() {
        let dir = TempDir::new("check").unwrap();
        let name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_ne!(name, format!("check-{}", std::process::id()));
        assert!(
            !dir.path()
                .starts_with(format!("/tmp/omarchy-sync-check-{}", std::process::id()))
        );
        let base = test_base("link");
        std::fs::create_dir_all(&base).unwrap();
        std::os::unix::fs::symlink(&base, base.with_extension("link")).unwrap();
        assert!(TempDir::new_in(&base.with_extension("link"), "t").is_err());
        let _ = std::fs::remove_file(base.with_extension("link"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn reads_curl_percentages() {
        assert_eq!(curl_percent("#########                  23.4%"), Some(23));
        assert_eq!(curl_percent("######################## 100.0%"), Some(100));
        assert_eq!(curl_percent(""), None);
        assert_eq!(
            curl_percent("curl: (22) The requested URL returned error: 404"),
            None
        );
    }

    #[test]
    fn reads_git_transfer_percentages() {
        assert_eq!(
            transfer_percent("Receiving objects:  45% (9/20), 1.2 MiB | 3 MiB/s"),
            Some(45)
        );
        assert_eq!(
            transfer_percent("Writing objects: 100% (3/3), done."),
            Some(100)
        );
        assert_eq!(transfer_percent("Compressing objects:  50% (1/2)"), None);
        assert_eq!(
            transfer_percent("remote: Enumerating objects: 5, done."),
            None
        );
    }

    #[test]
    fn friendly_picks_the_sentence_by_cause() {
        let verify = "attestation: gh attestation verify x: To get started with GitHub CLI, please run: gh auth login";
        assert!(friendly(verify).starts_with("Not signed in to GitHub"));
        assert!(friendly("checksum: sha256sum -c: x: FAILED").starts_with("The downloaded update"));
        assert!(friendly("0.1.2 isn't the latest release").starts_with("A newer release appeared"));
        assert!(friendly("curl: Could not resolve host").starts_with("Couldn't reach GitHub"));
    }
}
