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
    pub fn choices_changed(&self) -> bool {
        let read = |text: &str| {
            serde_yaml::from_str::<Data>(text).ok().map(|mut d| {
                d.upgrade();
                d.sorted()
            })
        };
        let saved = self.git(&["show", "HEAD:.chezmoidata.yaml"]).ok();
        let now = std::fs::read_to_string(self.data_path()).ok();
        saved.as_deref().and_then(read) != now.as_deref().and_then(read)
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
        let manifest = [
            self.home.join(PLUGIN_DIR).join(id),
            self.src.join("dot_config/omarchy/plugins").join(id),
        ]
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
        }
    }

    pub fn load_data(&self) -> Data {
        let mut data: Data = std::fs::read_to_string(self.data_path())
            .ok()
            .and_then(|s| serde_yaml::from_str(&s).ok())
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
    /// a newer version changed): saving over it would turn every choice off, on every computer.
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

/// A git command that talks to GitHub, passing its upload or download percentage on as it goes.
pub(crate) fn git_progress(
    ctx: &Ctx,
    args: &[&str],
    step: &dyn Fn(&str),
) -> Result<String, String> {
    use std::io::Read;
    let mut child = Command::new("git")
        .args(args)
        .current_dir(&ctx.src)
        .env("LC_ALL", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    let mut stderr = child.stderr.take().ok_or("git: no output")?;
    let (mut all, mut line, mut last) = (Vec::new(), Vec::new(), None);
    let mut buf = [0u8; 1024];
    // Progress lines end in \r as they update; each finished one in \n.
    while let Ok(n) = stderr.read(&mut buf)
        && n > 0
    {
        for &b in &buf[..n] {
            all.push(b);
            if b == b'\r' || b == b'\n' {
                let p = transfer_percent(&String::from_utf8_lossy(&line));
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
    let out = child.wait_with_output().map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        let err = String::from_utf8_lossy(&all);
        Err(format!("git {}: {}", args.join(" "), err.trim()))
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

/// Only one sync at a time, whether it comes from the window or the background timer.
pub(crate) fn lock(ctx: &Ctx) -> Result<std::fs::File, String> {
    let file =
        std::fs::File::create(ctx.src.join(".git/omarchy-sync.lock")).map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|_| "another sync is running".to_string())?;
    Ok(file)
}

/// Waits for any running sync (window or background timer) to finish, then holds the lock.
pub(crate) fn lock_wait(ctx: &Ctx) -> Result<std::fs::File, String> {
    let file =
        std::fs::File::create(ctx.src.join(".git/omarchy-sync.lock")).map_err(|e| e.to_string())?;
    file.lock().map_err(|e| e.to_string())?;
    Ok(file)
}

/// Loads the shared choices, applies one change and saves them, once any running sync (the
/// background timer's, say) is done: a switch flipped during a sync would otherwise be saved
/// over by it, then committed and pushed as the old value.
pub fn edit(ctx: &Ctx, change: impl FnOnce(&mut Data, &str)) -> Result<Data, String> {
    let _lock = lock_wait(ctx)?;
    ctx.data_readable()?;
    let mut data = ctx.load_data();
    change(&mut data, &ctx.host);
    ctx.save_data(&data)?;
    Ok(data)
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
