//! Omarchy Sync: a Tauri window over the sync engine in `sync.rs`.
//! The page in `ui/` calls the commands below; everything it sends is checked here,
//! because the commands install packages and change files.

mod sync;

use serde::Serialize;
use sync::{Ctx, Data, Overview, Prefer, Report, SetupState, SyncError};
use tauri::ipc::Channel;
use tauri::{Emitter, Manager};

fn main() {
    // The systemd timer runs the same binary headless.
    if std::env::args().any(|a| a == "--background") {
        sync::background(&Ctx::load());
        return;
    }
    tauri::Builder::default()
        // One window only: opening Omarchy Sync again (from the launcher or the bar indicator)
        // brings the open window forward on its Sync page, where anything waiting is listed.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            let _ = app.emit("show-sync", ());
        }))
        .setup(|app| {
            watch_look(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            look,
            folders,
            probe,
            overview,
            sync_now,
            resolve_conflict,
            undo,
            install_helpers,
            sign_in,
            start,
            join,
            install_apps,
            install_plugins,
            set_app_script,
            set_bar_indicator,
            add_repository,
            report,
            remove_app_script,
            install_with_script,
            choices_changed,
            set_plugin,
            set_app,
            add_apps,
            set_folder,
            folder_here,
            set_auto,
            set_install_auto,
            rename_computer,
            forget_computer,
            open_sync_space,
            disconnect,
        ])
        .run(tauri::generate_context!())
        .expect("Omarchy Sync couldn't start");
}

/// What the page shows when something fails: a sentence for people, and the raw error for whoever helps.
#[derive(Serialize)]
#[serde(tag = "kind")]
enum Problem {
    Failed { message: String, details: String },
    Conflict(sync::Conflict),
}

impl From<String> for Problem {
    fn from(e: String) -> Problem {
        Problem::Failed {
            message: sync::friendly(&e),
            details: e,
        }
    }
}

impl From<SyncError> for Problem {
    fn from(e: SyncError) -> Problem {
        match e {
            SyncError::Conflict(c) => Problem::Conflict(c),
            SyncError::Failed(e) => e.into(),
        }
    }
}

type Result<T> = std::result::Result<T, Problem>;

/// Git, chezmoi and pacman calls block, so they run off the UI thread.
async fn blocking<T: Send + 'static>(work: impl FnOnce(&Ctx) -> T + Send + 'static) -> T {
    tauri::async_runtime::spawn_blocking(move || work(&Ctx::load()))
        .await
        .expect("worker thread panicked")
}

fn steps(channel: Channel<String>) -> impl Fn(&str) {
    move |s: &str| {
        let _ = channel.send(s.to_string());
    }
}

/// What the page needs to dress itself like the desktop: the active Omarchy theme's colours
/// (the page derives contrast-checked roles from them) and whether desktop animations are on.
#[derive(Serialize)]
struct Look {
    /// The raw `colors.toml` of the active Omarchy theme, empty if there is none.
    colors_toml: String,
    /// Desktop animations are on (Hyprland and GNOME both allow them).
    motion: bool,
}

fn output(prog: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(prog)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).replace(' ', ""))
}

#[tauri::command]
fn look() -> Look {
    let hypr_off = output("hyprctl", &["getoption", "animations:enabled", "-j"])
        .is_some_and(|s| s.contains("\"bool\":false") || s.contains("\"int\":0"));
    let gnome_off = output(
        "gsettings",
        &["get", "org.gnome.desktop.interface", "enable-animations"],
    )
    .is_some_and(|s| s.trim() == "false");
    Look {
        colors_toml: colors_toml(&Ctx::load()),
        motion: !(hypr_off || gnome_off),
    }
}

fn colors_toml(ctx: &Ctx) -> String {
    let state = std::env::var("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or(ctx.home.join(".local/state"));
    std::fs::read_to_string(state.join("omarchy/current/theme/colors.toml")).unwrap_or_default()
}

/// Tells the page when the Omarchy theme or background changes, so it recolours itself and
/// lists the change to share, like other apps that follow the desktop theme.
fn watch_look(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let ctx = Ctx::load();
        let now = || {
            (
                ctx.current_theme(),
                colors_toml(&ctx),
                ctx.current_background(),
            )
        };
        let mut last = now();
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let seen = now();
            if seen != last {
                last = seen;
                let _ = app.emit("look-changed", ());
            }
        }
    });
}

#[derive(Serialize)]
struct Folder {
    path: &'static str,
    name: &'static str,
    /// Synced by default when setting up the first computer.
    default: bool,
    exists: bool,
}

#[derive(Serialize)]
struct Catalog {
    folders: Vec<Folder>,
    /// How a change to the app list and sync choices is named.
    data_label: &'static str,
}

/// The folders the app knows how to sync, with plain names for showing paths.
#[tauri::command]
fn folders() -> Catalog {
    let home = Ctx::load().home;
    let folders = sync::FOLDERS
        .iter()
        .map(|&(path, name, default)| Folder {
            path,
            name,
            default,
            exists: home.join(path).exists(),
        })
        .collect();
    Catalog {
        folders,
        data_label: sync::DATA_LABEL,
    }
}

#[tauri::command]
async fn probe() -> SetupState {
    blocking(sync::probe).await
}

#[derive(Serialize)]
struct Status {
    overview: Overview,
    data: Data,
    /// This computer's ID; names live in `data.computers`.
    host: String,
    /// The private GitHub repo everything syncs through, as a web address.
    sync_space: Option<String>,
    /// Set when the background timer couldn't be turned on or off.
    schedule_error: Option<String>,
    /// Whether the sync indicator is on the Omarchy bar.
    bar_indicator: bool,
}

#[tauri::command]
async fn overview(on_step: Channel<String>, just_synced: bool) -> Result<Status> {
    blocking(move |ctx| {
        let data = ctx.load_data();
        let manual = data.computers.get(&ctx.host).is_some_and(|c| c.manual);
        // Keeps the background timer matching this computer's choice (cheap when it already does).
        let schedule_error = sync::schedule(ctx, !manual).err();
        // On by default; a failure here only means no indicator, so it doesn't stop the check.
        let _ = sync::ensure_bar_indicator(ctx);
        let overview = sync::overview(ctx, &steps(on_step), just_synced)?;
        Ok(Status {
            overview,
            data: ctx.load_data(),
            host: ctx.host.clone(),
            sync_space: sync::sync_space(ctx),
            schedule_error,
            bar_indicator: sync::bar_indicator(ctx),
        })
    })
    .await
}

fn install_without_asking(ctx: &Ctx) -> bool {
    ctx.load_data()
        .computers
        .get(&ctx.host)
        .is_some_and(|c| c.install_without_asking)
}

#[tauri::command]
async fn sync_now(prefer: Prefer, on_step: Channel<String>) -> Result<Report> {
    blocking(move |ctx| sync::sync(ctx, prefer, install_without_asking(ctx), &steps(on_step)))
        .await
        .map_err(Problem::from)
}

/// Settles a clash with the version chosen for each clashing file (by its sync-folder path),
/// then syncs. Files that aren't clashing are simply not looked up.
#[tauri::command]
async fn resolve_conflict(
    choices: std::collections::BTreeMap<String, sync::Keep>,
    on_step: Channel<String>,
) -> Result<Report> {
    blocking(move |ctx| sync::resolve(ctx, &choices, install_without_asking(ctx), &steps(on_step)))
        .await
        .map_err(Problem::from)
}

#[tauri::command]
async fn undo(sha: String, on_step: Channel<String>) -> Result<Report> {
    if sha.len() != 40 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("not a history entry".to_string().into());
    }
    blocking(move |ctx| {
        let step = steps(on_step);
        step("Undoing");
        sync::undo(ctx, &sha).map_err(SyncError::Failed)?;
        sync::sync(ctx, Prefer::Ask, install_without_asking(ctx), &step)
    })
    .await
    .map_err(Problem::from)
}

/// Only the three tools setup needs; the page can't ask pacman for anything else.
#[tauri::command]
async fn install_helpers() -> Result<()> {
    blocking(|ctx| {
        let missing = sync::probe(ctx).missing;
        if missing.is_empty() {
            Ok(())
        } else {
            sync::install_helpers(&missing)
        }
    })
    .await
    .map_err(Problem::from)
}

#[tauri::command]
fn sign_in() -> Result<()> {
    sync::sign_in().map_err(Problem::from)
}

#[tauri::command]
async fn start(on_step: Channel<String>) -> Result<()> {
    blocking(move |ctx| sync::start(ctx, &steps(on_step)))
        .await
        .map_err(Problem::from)
}

/// Returns where this computer's previous settings were backed up.
#[tauri::command]
async fn join(on_step: Channel<String>) -> Result<String> {
    blocking(move |ctx| {
        let login = sync::run(None, "gh", &["api", "user", "--jq", ".login"])?;
        sync::join(ctx, &login, &steps(on_step)).map(|p| p.display().to_string())
    })
    .await
    .map_err(Problem::from)
}

/// Installs exactly the synced apps this computer is missing, nothing the page names itself.
#[tauri::command]
async fn install_apps() -> Result<()> {
    blocking(|ctx| {
        let missing = sync::missing_apps(&ctx.load_data(), &ctx.host, &sync::installed());
        if missing.is_empty() {
            Ok(())
        } else {
            sync::install_apps(&missing)
        }
    })
    .await
    .map_err(Problem::from)
}

/// Adds exactly the synced plugins this computer is waiting for, nothing the page names itself.
#[tauri::command]
async fn install_plugins() -> Result<Data> {
    blocking(move |ctx| {
        let ids = ctx.load_data().missing_plugins(&ctx.host);
        sync::install_plugins(ctx, &ids).map(|()| ctx.load_data())
    })
    .await
    .map_err(Problem::from)
}

fn plugin_id(id: &str) -> Result<()> {
    if sync::is_plugin_id(id) {
        Ok(())
    } else {
        Err(format!("not a plugin: {id}").into())
    }
}

/// Whether one computer gets a plugin; see `Data::place`.
#[tauri::command]
async fn set_plugin(computer: String, plugin: String, on: bool) -> Result<Data> {
    plugin_id(&plugin)?;
    let mut shared = None;
    let data = edit(|d, host| {
        let before = d.plugins.contains(&plugin);
        // This computer's copy may be the one shared, so it counts as having what's here.
        d.computers.entry(host.into()).or_default().plugins = Ctx::load().local_plugins();
        d.place_plugin(&computer, &plugin, on);
        let after = d.plugins.contains(&plugin);
        if before != after {
            shared = Some(after);
        }
    })?;
    if let Some(on) = shared {
        blocking(move |ctx| sync::track_plugin(ctx, &plugin, on)).await?;
    }
    Ok(data)
}

/// Adds an extra package repository on this computer, as another computer has it (after the
/// person confirmed); only one this computer needs and lacks, by its recorded name.
#[tauri::command]
async fn add_repository(name: String) -> Result<()> {
    blocking(move |ctx| {
        let data = ctx.load_data();
        let needed = sync::missing_repos(&data, &ctx.host, &sync::installed(), &sync::repos_here());
        if !needed.iter().any(|r| r.name == name) {
            return Err(format!("{name} isn't a repository this computer needs"));
        }
        sync::add_repository(&data, &name)
    })
    .await
    .map_err(Problem::from)
}

/// Shows the sync indicator on the Omarchy bar, or takes it off.
#[tauri::command]
async fn set_bar_indicator(on: bool) -> Result<()> {
    // Remembered per computer, so the app doesn't put it back.
    edit(|d, host| d.computers.entry(host.into()).or_default().no_bar_indicator = !on)?;
    blocking(move |ctx| sync::set_bar_indicator(ctx, on))
        .await
        .map_err(Problem::from)
}

#[tauri::command]
fn choices_changed() -> bool {
    Ctx::load().choices_changed()
}

/// Loads the shared data, applies one change, saves it, and hands the result back to the page.
fn edit(change: impl FnOnce(&mut Data, &str)) -> Result<Data> {
    let ctx = Ctx::load();
    let mut data = ctx.load_data();
    change(&mut data, &ctx.host);
    ctx.save_data(&data)?;
    Ok(data)
}

#[tauri::command]
fn set_app(computer: String, app: String, on: bool) -> Result<Data> {
    let ctx = Ctx::load();
    // An app from outside the repos syncs only through its install script.
    if on && ctx.load_data().outside.get(&app) == Some(&None) {
        return Err(format!(
            "{app} isn't in Arch's repos or the AUR, so add an install script before syncing it."
        )
        .into());
    }
    let installed_here = sync::installed().contains(&app);
    edit(|d, host| d.place_app(host, installed_here, &computer, &app, on))
}

/// Saves how to install an app from outside Arch's repos and the AUR: one command, run in an
/// empty folder in a terminal window.
#[tauri::command]
fn set_app_script(app: String, command: String) -> Result<Data> {
    // One step per line; Windows line ends become plain ones.
    let command = command.replace("\r\n", "\n").trim().to_string();
    if !sync::is_package_name(&app) {
        return Err(format!("not an app: {app}").into());
    }
    if command.is_empty()
        || command.chars().count() > 4000
        || command.lines().count() > 50
        || command
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(
            "The install steps need 1 to 50 lines, up to 4,000 characters."
                .to_string()
                .into(),
        );
    }
    let ctx = Ctx::load();
    if !ctx.load_data().outside.contains_key(&app) && !sync::installed().contains(&app) {
        return Err(format!("{app} isn't installed here.").into());
    }
    edit(|d, _| {
        d.outside.insert(
            app,
            Some(sync::Script {
                command,
                ..Default::default()
            }),
        );
    })
}

/// Forgets an app's install script; it then stays on the computers that have it.
#[tauri::command]
fn remove_app_script(app: String) -> Result<Data> {
    edit(|d, _| {
        if let Some(script) = d.outside.get_mut(&app) {
            *script = None;
        }
    })
}

/// Installs one waiting app with its script, in a terminal window; only an app this computer
/// is really waiting for, with the script saved for it (nothing the page names itself).
#[tauri::command]
async fn install_with_script(app: String, on_step: Channel<String>) -> Result<()> {
    blocking(move |ctx| {
        let waiting = sync::missing_scripted(&ctx.load_data(), &ctx.host, &sync::installed());
        let (_, script) = waiting
            .iter()
            .find(|(a, _)| *a == app)
            .ok_or_else(|| format!("{app} isn't waiting to be installed here"))?;
        sync::install_with_script(ctx, &app, script, &steps(on_step))
    })
    .await
    .map_err(Problem::from)
}

#[tauri::command]
fn add_apps(apps: Vec<String>) -> Result<Data> {
    edit(|d, _| {
        // Apps from outside the repos without an install script can't reach other computers.
        for a in apps.iter().filter(|a| d.outside.get(*a) != Some(&None)) {
            sync::toggle(&mut d.apps, a, true);
        }
    })
}

/// Folders are limited to the known list; the page can't make chezmoi add arbitrary paths.
fn known_folder(path: &str) -> Result<&'static str> {
    sync::FOLDERS
        .iter()
        .map(|f| f.0)
        .find(|f| *f == path)
        .ok_or_else(|| format!("unknown folder {path}").into())
}

#[tauri::command]
async fn set_folder(path: String, on: bool) -> Result<Data> {
    let path = known_folder(&path)?;
    // Stop tracking before saving "off": once saved, the ignore rules hide it from chezmoi,
    // which then can't forget it. Starting is the other way round.
    if !on {
        blocking(move |ctx| sync::track(ctx, path, false)).await?;
    }
    let data = edit(|d, _| sync::toggle(&mut d.folders, path, on))?;
    if on {
        blocking(move |ctx| sync::track(ctx, path, true)).await?;
    }
    Ok(data)
}

#[tauri::command]
fn folder_here(path: String, on: bool) -> Result<Data> {
    let path = known_folder(&path)?;
    edit(|d, host| d.set_folder_here(host, path, on))
}

#[tauri::command]
async fn set_auto(on: bool) -> Result<Data> {
    let data = edit(|d, host| d.computers.entry(host.into()).or_default().manual = !on)?;
    blocking(move |ctx| sync::schedule(ctx, on)).await?;
    Ok(data)
}

#[tauri::command]
fn set_install_auto(on: bool) -> Result<Data> {
    edit(|d, host| {
        d.computers
            .entry(host.into())
            .or_default()
            .install_without_asking = on
    })
}

/// Each computer names itself; the name is shared with the others on the next sync.
#[tauri::command]
fn rename_computer(name: String) -> Result<Data> {
    let name = name.trim().to_string();
    if name.is_empty() || name.chars().count() > 40 || name.chars().any(char::is_control) {
        return Err("A computer name needs 1 to 40 characters."
            .to_string()
            .into());
    }
    // Names tell computers apart in History and on the Sync screen, so each is used once.
    let ctx = Ctx::load();
    if sync::name_taken(&ctx.load_data(), &ctx.host, &name) {
        return Err(
            format!("Another computer is already called {name}. Choose a different name.").into(),
        );
    }
    edit(|d, host| d.computers.entry(host.into()).or_default().name = name)
}

/// Takes a retired computer off the list. If it ever syncs again it simply reappears.
#[tauri::command]
fn forget_computer(id: String) -> Result<Data> {
    let ctx = Ctx::load();
    if id == ctx.host || !ctx.load_data().computers.contains_key(&id) {
        return Err(format!("not another known computer: {id}").into());
    }
    edit(|d, _| {
        d.computers.remove(&id);
    })
}

/// Opens this computer's sync space on GitHub. The address comes from the repo, not the page.
#[tauri::command]
async fn open_sync_space() -> Result<()> {
    blocking(|ctx| {
        let url = sync::sync_space(ctx).ok_or("This computer isn't connected to GitHub.")?;
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(Problem::from)
}

/// Where problems and ideas are reported.
const ISSUES: &str = "https://github.com/tsubaie/OmaSync/issues/new";

/// Opens a new GitHub issue in the browser: with no message, the choice of kind (problem,
/// suggestion, something else); with an error's message and details, a problem report filled
/// in with them. The person reads it on GitHub and sends it themselves. The home folder in the
/// details becomes `~`, and the text is trimmed to what fits in a web address.
#[tauri::command]
fn report(message: Option<String>, details: Option<String>) -> Result<()> {
    let clean = |s: &str, max: usize| -> String {
        let home = std::env::var("HOME").unwrap_or_default();
        let s = if home.len() > 1 {
            s.replace(&home, "~")
        } else {
            s.to_string()
        };
        s.chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(max)
            .collect()
    };
    let url = match message {
        None => format!("{ISSUES}/choose"),
        Some(message) => {
            let message = clean(&message, 500);
            let title: String = message.chars().take(100).collect();
            format!(
                "{ISSUES}?template=bug.yml&title={}&what-happened={}&details={}&version={}",
                encode(&title),
                encode(&message),
                encode(&clean(&details.unwrap_or_default(), 4000)),
                encode(env!("CARGO_PKG_VERSION")),
            )
        }
    };
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string().into())
}

/// Percent-encodes text for a web address's query.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Stops syncing this computer. Returns where the local sync folder was moved.
#[tauri::command]
async fn disconnect() -> Result<String> {
    blocking(|ctx| sync::disconnect(ctx).map(|p| p.display().to_string()))
        .await
        .map_err(Problem::from)
}
