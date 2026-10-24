//! Everything that touches chezmoi, git, gh and pacman. No UI code here.

mod apps;
mod background;
mod bar;
mod choices;
mod ctx;
mod data;
mod history;
mod rules;
mod setup;

pub use apps::{
    add_repository, install_apps, install_with_script, installed, is_package_name, missing_apps,
    missing_repos, missing_scripted, repos_here,
};
pub use background::{background, schedule};
pub use bar::{bar_indicator, ensure_bar_indicator, set_bar_indicator};
pub use ctx::{Ctx, edit, friendly, run, sync_space};
// What the updater shares with a sync.
pub(crate) use ctx::{lock, lock_wait, plan};
pub use data::{Data, Script, is_plugin_id, name_taken, toggle};
pub use history::undo;
pub use rules::{DATA_LABEL, FOLDERS};
pub use setup::{SetupState, install_helpers, join, probe, sign_in, start};

// Shared by the modules, each of which starts with `use super::*`.
use apps::*;
use background::*;
use bar::*;
use ctx::*;
use data::*;
use history::*;
use rules::*;
use serde::{Deserialize, Serialize};
use setup::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
pub(crate) fn save_newly_synced(
    ctx: &Ctx,
    synced_before: &[String],
) -> Result<Option<String>, String> {
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
pub(crate) fn strip_attributes(name: &str) -> String {
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

pub(crate) fn stage(ctx: &Ctx, data: &Data, step: &dyn Fn(&str)) -> Result<(), String> {
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

pub(crate) fn commit(ctx: &Ctx) -> Result<(), String> {
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
pub(crate) fn note_omarchy(data: &mut Data, host: &str) {
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
