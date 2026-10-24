//! The background timer: the systemd unit, and what it tells the person.

use super::*;

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
    // Once a day at midnight; a computer that was off then catches up when it next starts.
    let timer = "[Unit]\nDescription=Sync Omarchy settings once a day\n\n[Timer]\nOnCalendar=daily\nPersistent=true\n\n[Install]\nWantedBy=timers.target\n";
    let dir = unit_dir(ctx);
    let service_path = dir.join(format!("{UNIT}.service"));
    let timer_path = dir.join(format!("{UNIT}.timer"));
    let current = |path: &Path, text: &str| std::fs::read_to_string(path).is_ok_and(|s| s == text);
    if enabled && current(&service_path, &service) && current(&timer_path, timer) {
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&service_path, service).map_err(|e| e.to_string())?;
    std::fs::write(&timer_path, timer).map_err(|e| e.to_string())?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
