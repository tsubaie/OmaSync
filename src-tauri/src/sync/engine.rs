//! A sync: get the other computers' changes, settle clashes, pause for an older Omarchy, apply, share.

use super::*;

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
}
