//! A lab for sync scenarios: any number of computers on this machine, each a temporary home, a
//! bare repository standing in for GitHub, and fakes on PATH for every tool the engine calls
//! (omarchy, pacman, systemctl, …), so nothing reaches the real desktop.
//!
//! After every step the lab checks the promise the app makes, on the computer that acted:
//! 1. a synced file holds the version in this computer's history (or just put in the sync
//!    folder by a switch), or the person's own change that no sync here has saved yet;
//! 2. nothing the person wrote is replaced or removed unless that exact version is in the
//!    history or in a backup;
//! 3. a version that reached the history stays there for good (or is still the file on this
//!    computer, or in a backup, when a switch took its folder out of the sync);
//! 4. settings that don't sync here are never touched.
//!
//! Scenarios run one at a time, because chezmoi and git find their folders through environment
//! variables, which are process-wide.

use super::*;
use std::sync::{Mutex, MutexGuard};

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

pub struct Lab {
    pub root: PathBuf,
    pub remote: String,
    original_path: String,
    machines: Vec<Machine>,
    _guard: MutexGuard<'static, ()>,
}

struct Machine {
    name: String,
    ctx: Ctx,
    omarchy: String,
    packages: String,
    /// The last thing the lab put at each path under home (content and its git ID; None:
    /// removed).
    written: BTreeMap<String, Option<(Vec<u8>, String)>>,
    /// Versions (git ID → path and content) seen in this computer's history.
    saved: BTreeMap<String, (String, Vec<u8>)>,
    /// Settings files that don't sync here, as they were at the last check.
    untouched: BTreeMap<String, Option<Vec<u8>>>,
}

const FAKES: &[(&str, &str)] = &[
    (
        "omarchy",
        r#"echo "omarchy $*" >> "$LAB_LOG"
case "$1 $2" in
  "version ") echo "$LAB_OMARCHY_VERSION" ;;
  "theme set") mkdir -p "$XDG_STATE_HOME/omarchy/current"; echo "$3" > "$XDG_STATE_HOME/omarchy/current/theme.name" ;;
  "theme bg") mkdir -p "$XDG_STATE_HOME/omarchy/current"; ln -sfn "$4" "$XDG_STATE_HOME/omarchy/current/background" ;;
esac"#,
    ),
    (
        "pacman",
        r#"case "$1" in
  -Qq|-Qqe) printf '%s\n' $LAB_PACKAGES ;;
esac"#,
    ),
    ("pacman-conf", "printf 'omarchy\\ncore\\nextra\\n'"),
    ("yay", r#"echo "yay $*" >> "$LAB_LOG""#),
    ("pkexec", r#"echo "pkexec $*" >> "$LAB_LOG""#),
    ("notify-send", r#"echo "notify-send $*" >> "$LAB_LOG""#),
    ("curl", "exit 1"),
    (
        "systemctl",
        r#"echo "systemctl $*" >> "$LAB_LOG"
case "$*" in *is-enabled*) echo disabled; exit 1 ;; esac"#,
    ),
    // Fails one git operation on demand (LAB_FAIL_GIT names it: fetch, push, …): a dropped
    // connection, GitHub down, a crash part way.
    (
        "git",
        r#"case " $* " in
  *" $LAB_FAIL_GIT "*) [ -n "$LAB_FAIL_GIT" ] && { echo "lab: git $LAB_FAIL_GIT failed" >&2; exit 1; } ;;
esac
exec "$LAB_REAL_GIT" "$@""#,
    ),
];

/// Runs a command for its raw output, untrimmed (file contents end in newlines).
fn raw(dir: &Path, prog: &str, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new(prog)
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// git's ID for some content, so "is this version in the history" is one lookup.
fn blob_id(dir: &Path, content: &[u8]) -> String {
    let file = dir.join("lab-blob");
    std::fs::write(&file, content).unwrap();
    let id = run(Some(dir), "git", &["hash-object", "lab-blob"]).unwrap();
    let _ = std::fs::remove_file(file);
    id
}

impl Lab {
    pub fn new() -> Lab {
        let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
        let root =
            std::env::temp_dir().join(format!("omarchy-sync-lab-{}-{}", std::process::id(), now()));
        let _ = std::fs::remove_dir_all(&root);
        let remote = root.join("github.git");
        std::fs::create_dir_all(&remote).unwrap();
        run(Some(&remote), "git", &["init", "-q", "--bare"]).unwrap();
        let original_path = std::env::var("PATH").unwrap_or_default();
        let real_git = original_path
            .split(':')
            .map(|d| Path::new(d).join("git"))
            .find(|p| p.is_file())
            .expect("git on PATH");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for (name, body) in FAKES {
            let path = bin.join(name);
            std::fs::write(&path, format!("#!/bin/bash\n{body}\n")).unwrap();
            let mut perm = std::fs::metadata(&path).unwrap().permissions();
            std::os::unix::fs::PermissionsExt::set_mode(&mut perm, 0o755);
            std::fs::set_permissions(&path, perm).unwrap();
        }
        unsafe {
            std::env::set_var("PATH", format!("{}:{original_path}", bin.display()));
            std::env::set_var("LAB_REAL_GIT", real_git);
            std::env::remove_var("LAB_FAIL_GIT");
        }
        Lab {
            remote: remote.to_string_lossy().into_owned(),
            root,
            original_path,
            machines: Vec::new(),
            _guard: guard,
        }
    }

    /// A computer called `name` (its hostname too), with a few settings of its own: keyboard
    /// shortcuts, a screen layout, and a custom theme "mine" in use.
    pub fn add(&mut self, name: &str) {
        self.add_named(name, name);
    }

    pub fn add_named(&mut self, id: &str, hostname: &str) {
        let home = self.root.join(id);
        let ctx = Ctx {
            src: home.join(".local/share/chezmoi"),
            host: id.into(),
            hostname: hostname.into(),
            home,
        };
        self.machines.push(Machine {
            name: id.into(),
            ctx,
            omarchy: "4.0.0".into(),
            packages: String::new(),
            written: BTreeMap::new(),
            saved: BTreeMap::new(),
            untouched: BTreeMap::new(),
        });
        self.write(id, ".config/hypr/bindings.lua", format!("-- {id} keys\n"));
        self.write(id, ".config/hypr/monitors.lua", format!("-- {id} screen\n"));
        self.write(
            id,
            ".config/omarchy/themes/mine/colors.toml",
            "accent = 1\n",
        );
        self.write(id, ".local/state/omarchy/current/theme.name", "mine\n");
    }

    fn index(&self, name: &str) -> usize {
        self.machines
            .iter()
            .position(|m| m.name == name)
            .unwrap_or_else(|| panic!("no computer {name}"))
    }

    /// Makes `name` the current computer for chezmoi, git and the engine.
    fn enter(&self, name: &str) -> &Machine {
        let m = &self.machines[self.index(name)];
        let home = &m.ctx.home;
        let _ = std::fs::create_dir_all(home.join(".lab"));
        unsafe {
            std::env::set_var("HOME", home);
            std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
            std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
            std::env::set_var("XDG_STATE_HOME", home.join(".local/state"));
            std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
            std::env::set_var("OMARCHY_SYNC_HOST", &m.ctx.host);
            std::env::set_var("LAB_OMARCHY_VERSION", &m.omarchy);
            std::env::set_var("LAB_PACKAGES", &m.packages);
            std::env::set_var("LAB_LOG", home.join(".lab/calls"));
        }
        m
    }

    /// A computer's context, with the environment switched to it, so the engine can be called
    /// directly for what the lab has no helper for.
    pub fn ctx(&self, name: &str) -> &Ctx {
        &self.enter(name).ctx
    }

    /// What the fake tools were asked to do on this computer, one call per line.
    pub fn calls(&self, name: &str) -> String {
        std::fs::read_to_string(self.ctx(name).home.join(".lab/calls")).unwrap_or_default()
    }

    pub fn omarchy_version(&mut self, name: &str, version: &str) {
        let i = self.index(name);
        self.machines[i].omarchy = version.into();
    }

    /// The apps installed on this computer, as pacman would list them.
    pub fn packages(&mut self, name: &str, packages: &str) {
        let i = self.index(name);
        self.machines[i].packages = packages.into();
    }

    /// Makes every git `op` (fetch, push, …) fail on every computer, or none.
    pub fn fail_git(&self, op: Option<&str>) {
        unsafe {
            match op {
                Some(op) => std::env::set_var("LAB_FAIL_GIT", op),
                None => std::env::remove_var("LAB_FAIL_GIT"),
            }
        }
    }

    // ---------- Files ----------

    /// Puts a file on a computer, as the person would.
    pub fn write(&mut self, name: &str, path: &str, content: impl AsRef<[u8]>) {
        let i = self.index(name);
        let file = self.machines[i].ctx.home.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, content.as_ref()).unwrap();
        let id = blob_id(&self.root, content.as_ref());
        let m = &mut self.machines[i];
        m.written
            .insert(path.into(), Some((content.as_ref().to_vec(), id)));
        if let Some(seen) = m.untouched.get_mut(path) {
            *seen = Some(content.as_ref().to_vec());
        }
    }

    pub fn remove(&mut self, name: &str, path: &str) {
        let i = self.index(name);
        let file = self.machines[i].ctx.home.join(path);
        let _ = std::fs::remove_dir_all(&file).or_else(|_| std::fs::remove_file(&file));
        let m = &mut self.machines[i];
        m.written.insert(path.into(), None);
        if let Some(seen) = m.untouched.get_mut(path) {
            *seen = None;
        }
    }

    pub fn read(&self, name: &str, path: &str) -> String {
        String::from_utf8_lossy(&self.bytes(name, path)).into_owned()
    }

    pub fn bytes(&self, name: &str, path: &str) -> Vec<u8> {
        std::fs::read(self.ctx(name).home.join(path))
            .unwrap_or_else(|e| panic!("{name}: {path}: {e}"))
    }

    pub fn exists(&self, name: &str, path: &str) -> bool {
        self.ctx(name).home.join(path).exists()
    }

    // ---------- The engine ----------

    /// The first computer: creates the sync space, as Setup's "Save this computer's settings".
    pub fn start(&mut self, name: &str) {
        let ctx = self.ctx(name).clone();
        seed(&ctx).unwrap();
        ctx.git(&["remote", "add", "origin", &self.remote]).unwrap();
        self.sync_ok(name);
    }

    /// Another computer takes the synced settings ("Use My Synced Settings"), then syncs.
    /// Returns its backup.
    pub fn join(&mut self, name: &str) -> PathBuf {
        let ctx = self.ctx(name).clone();
        let backup = adopt(&ctx, &self.remote, &|_| {}).unwrap();
        self.check(name);
        self.sync_ok(name);
        backup
    }

    pub fn sync(&mut self, name: &str) -> Result<Report, SyncError> {
        let ctx = self.ctx(name).clone();
        let result = sync(&ctx, Prefer::Ask, false, &|_| {});
        self.check(name);
        result
    }

    pub fn sync_ok(&mut self, name: &str) -> Report {
        match self.sync(name) {
            Ok(r) => r,
            Err(SyncError::Failed(e)) => panic!("{name}: sync failed: {e}"),
            Err(SyncError::Conflict(c)) => panic!("{name}: unexpected clash: {c:?}"),
        }
    }

    pub fn sync_fails(&mut self, name: &str) -> String {
        match self.sync(name) {
            Err(SyncError::Failed(e)) => e,
            other => panic!("{name}: expected the sync to fail, got {other:?}"),
        }
    }

    pub fn clash(&mut self, name: &str) -> Conflict {
        match self.sync(name) {
            Err(SyncError::Conflict(c)) => c,
            other => panic!("{name}: expected a clash, got {other:?}"),
        }
    }

    /// Settles a clash with a choice per file (the path under home), then syncs.
    pub fn resolve(
        &mut self,
        name: &str,
        clash: &Conflict,
        choose: impl Fn(&str) -> Keep,
    ) -> Result<Report, SyncError> {
        let choices = clash
            .versions
            .iter()
            .map(|v| (v.source.clone(), choose(&v.path)))
            .collect();
        let ctx = self.ctx(name).clone();
        let result = resolve(&ctx, &choices, false, &|_| {});
        self.check(name);
        result
    }

    pub fn history(&self, name: &str) -> Vec<Entry> {
        history(self.ctx(name))
    }

    /// Undoes the history entry at `index` (0 is the latest); the scenario then syncs, as the
    /// page does.
    pub fn undo(&mut self, name: &str, index: usize) -> Result<(), String> {
        let sha = self.history(name)[index].sha.clone();
        let r = undo(self.ctx(name), &sha);
        self.check(name);
        r
    }

    pub fn data(&self, name: &str) -> Data {
        self.ctx(name).load_data()
    }

    /// Changes the shared choices, as the page's commands do.
    pub fn edit(&mut self, name: &str, change: impl FnOnce(&mut Data, &str)) {
        edit(self.ctx(name), change).unwrap();
    }

    /// A folder's switch, in the order the `set_folder` command uses.
    pub fn set_folder(&mut self, name: &str, folder: &str, on: bool) -> Result<(), String> {
        let ctx = self.ctx(name).clone();
        let mut result = Ok(());
        if !on {
            result = track(&ctx, folder, false);
        }
        if result.is_ok() {
            self.edit(name, |d, _| toggle(&mut d.folders, folder, on));
            if on {
                result = track(&ctx, folder, true);
            }
        }
        self.check(name);
        result
    }

    pub fn head(&self, name: &str) -> String {
        self.ctx(name).git(&["rev-parse", "HEAD"]).unwrap()
    }

    /// Every computer syncs until a whole round changes nothing, and all hold the same history.
    pub fn settle(&mut self) {
        let names: Vec<String> = self.machines.iter().map(|m| m.name.clone()).collect();
        let heads = |lab: &Lab| -> BTreeSet<String> { names.iter().map(|n| lab.head(n)).collect() };
        let mut before = heads(self);
        for _ in 0..6 {
            for n in &names {
                self.sync_ok(n);
            }
            let after = heads(self);
            if after.len() == 1 && after == before {
                return;
            }
            before = after;
        }
        panic!("computers didn't settle: {before:?}");
    }

    /// Replaces the sync space with one saved earlier (a tarball of the bare repository).
    pub fn restore_remote(&self, tarball: &Path) {
        let _ = std::fs::remove_dir_all(&self.remote);
        std::fs::create_dir_all(&self.remote).unwrap();
        let tar = tarball.to_string_lossy();
        run(Some(Path::new(&self.remote)), "tar", &["-xzf", &tar]).unwrap();
    }

    /// Saves the sync space as a tarball, for `restore_remote` in a later version.
    pub fn save_remote(&self, tarball: &Path) {
        let tar = tarball.to_string_lossy();
        run(Some(Path::new(&self.remote)), "tar", &["-czf", &tar, "."]).unwrap();
    }

    /// Stops syncing here (Settings → Disconnect): the history goes with the sync folder.
    pub fn disconnect(&mut self, name: &str) {
        disconnect(self.ctx(name)).unwrap();
        let i = self.index(name);
        self.machines[i].saved.clear();
        self.check(name);
    }

    // ---------- The promise ----------

    /// The files chezmoi syncs on this computer: path under home → path in the sync folder.
    fn managed(&self, name: &str) -> BTreeMap<String, String> {
        self.enter(name);
        let out = run(
            None,
            "chezmoi",
            &["managed", "--include=files", "--path-style=all"],
        )
        .unwrap_or_default();
        serde_json::from_str::<serde_json::Value>(&out)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(target, v)| Some((target, v["sourceRelative"].as_str()?.to_string())))
            .collect()
    }

    /// Checks the promise on one computer. Called after every step; scenarios can call it too.
    pub fn check(&mut self, name: &str) {
        let managed = self.managed(name);
        let i = self.index(name);
        let m = &mut self.machines[i];
        let (home, src) = (m.ctx.home.clone(), m.ctx.src.clone());
        let disk = |path: &str| std::fs::read(home.join(path)).ok();
        let in_history: BTreeSet<String> = raw(&src, "git", &["rev-list", "--all", "--objects"])
            .map(|out| {
                String::from_utf8_lossy(&out)
                    .lines()
                    .filter_map(|l| l.split(' ').next().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let backups = home.join(".local/share/omarchy-sync/backups");
        let in_backups = |path: &str, content: &[u8]| {
            std::fs::read_dir(&backups)
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| {
                    let tar = e.path().to_string_lossy().into_owned();
                    raw(&home, "tar", &["-xOzf", &tar, path]).as_deref() == Some(content)
                })
        };
        for (path, source) in &managed {
            let on_disk = disk(path);
            let committed = raw(&src, "git", &["show", &format!("HEAD:{source}")]);
            let staged = std::fs::read(src.join(source)).ok();
            // The person's own change here, if any (None inside: they removed the file).
            let own = m
                .written
                .get(path)
                .map(|w| w.as_ref().map(|(c, _)| c.clone()));
            let show = |v: &Option<Vec<u8>>| match v {
                Some(b) => String::from_utf8_lossy(b)
                    .chars()
                    .take(60)
                    .collect::<String>(),
                None => "(none)".into(),
            };
            assert!(
                on_disk == committed
                    || on_disk == staged
                    || own.as_ref().is_some_and(|o| *o == on_disk),
                "{name}: {path} holds a version that is in neither its history nor the person's own change\n  on disk: {:?}\n  in history: {:?}\n  own change: {:?}",
                show(&on_disk),
                show(&committed),
                own.as_ref().map(show).unwrap_or_default()
            );
        }
        for (path, wrote) in &m.written {
            let settings = FOLDERS
                .iter()
                .any(|(f, _, _)| path == f || path.starts_with(&format!("{f}/")));
            if !settings {
                continue;
            }
            if let Some((content, id)) = wrote {
                if in_history.contains(id) {
                    m.saved.insert(id.clone(), (path.clone(), content.clone()));
                }
                assert!(
                    disk(path).as_deref() == Some(content)
                        || in_history.contains(id)
                        || in_backups(path, content),
                    "{name}: {path} was replaced, and the person's version is in neither the history nor a backup"
                );
            }
            if managed.contains_key(path) {
                m.untouched.remove(path);
            } else {
                let now = disk(path);
                if let Some(before) = m.untouched.get(path) {
                    assert!(
                        *before == now,
                        "{name}: {path} doesn't sync here but changed"
                    );
                }
                m.untouched.insert(path.clone(), now);
            }
        }
        for (id, (path, content)) in &m.saved {
            assert!(
                in_history.contains(id)
                    || disk(path).as_deref() == Some(content)
                    || in_backups(path, content),
                "{name}: a version of {path} that was in the history is nowhere now"
            );
        }
    }
}

impl Drop for Lab {
    fn drop(&mut self) {
        unsafe {
            std::env::set_var("PATH", &self.original_path);
            std::env::remove_var("LAB_FAIL_GIT");
        }
        if std::env::var("LAB_KEEP").is_err() {
            let _ = std::fs::remove_dir_all(&self.root);
        } else {
            eprintln!("lab kept at {}", self.root.display());
        }
    }
}
