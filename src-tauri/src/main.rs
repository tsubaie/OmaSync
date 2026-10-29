//! Omarchy Sync: a Tauri window over the sync engine in `sync/`.
//! The page in `ui/` calls the commands below; everything it sends is checked here,
//! because the commands install packages and change files.

mod macos_theme;
mod sync;
mod update;

use serde::Serialize;
use sync::{Ctx, Data, Overview, Report, SetupState, SnapshotOutcome, SyncError};
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
            restore_backup,
            install_apps,
            install_plugins,
            set_app_script,
            set_bar_indicator,
            macos_theme_status,
            macos_theme_apply,
            macos_theme_others,
            macos_theme_update,
            add_repository,
            try_omarchy,
            put_back_omarchy,
            update_omarchy,
            report,
            remove_app_script,
            install_with_script,
            choices_changed,
            set_plugin,
            set_plugin_enabled,
            add_plugin,
            update_plugins,
            remove_plugin,
            set_app,
            app_here,
            plugin_here,
            set_folder,
            folder_here,
            set_auto,
            set_install_auto,
            rename_computer,
            forget_computer,
            open_sync_space,
            disconnect,
            check_update,
            install_update,
            open_release,
            open_page,
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
    std::fs::read_to_string(ctx.state_dir().join("omarchy/current/theme/colors.toml"))
        .unwrap_or_default()
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
    /// The app whose settings it holds (by package name), for app-data folders.
    app: Option<&'static str>,
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
    let folders = sync::folders_for_profile(&sync::here().profile())
        .into_iter()
        .map(|(path, name, default)| Folder {
            path,
            name,
            default,
            exists: home.join(path).exists(),
            app: sync::folder_app(path),
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
    /// This copy's version, and how it was installed (which decides how it updates).
    version: &'static str,
    installed_how: update::How,
}

#[tauri::command]
async fn overview(on_step: Channel<String>, just_synced: bool) -> Result<Status> {
    blocking(move |ctx| {
        let data = ctx.load_data();
        let manual = data.computers.get(&ctx.host).is_some_and(|c| c.manual);
        // Keeps the background timer matching this computer's choice (cheap when it already does).
        let schedule_error = sync::schedule(ctx, !manual).err();
        // On by default; a failure here only means no indicator, so it doesn't stop the check.
        let _ = sync::ensure_bar_indicator(ctx, &data);
        let overview = sync::overview(ctx, &data, &steps(on_step), just_synced)?;
        Ok(Status {
            overview,
            data,
            host: ctx.host.clone(),
            sync_space: sync::sync_space(ctx),
            schedule_error,
            bar_indicator: sync::bar_indicator(ctx),
            version: env!("CARGO_PKG_VERSION"),
            installed_how: update::installed_how(),
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
async fn sync_now(on_step: Channel<String>) -> Result<Report> {
    blocking(move |ctx| sync::sync(ctx, install_without_asking(ctx), &steps(on_step)))
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
        sync::sync(ctx, install_without_asking(ctx), &step)
    })
    .await
    .map_err(Problem::from)
}

/// Only the three tools setup needs; the page can't ask the package manager for anything else.
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

/// The folders the page chose, each one the app knows.
fn known_folders(paths: Vec<String>) -> Result<Vec<String>> {
    paths
        .iter()
        .map(|p| known_folder(p).map(String::from))
        .collect()
}

/// `folders`, `apps`, `plugins`: the ones chosen to sync (None: the defaults).
#[tauri::command]
async fn start(
    folders: Option<Vec<String>>,
    apps: Option<Vec<String>>,
    plugins: Option<Vec<String>>,
    on_step: Channel<String>,
) -> Result<()> {
    let folders = folders.map(known_folders).transpose()?;
    apps.iter().flatten().try_for_each(|a| app_name(a))?;
    plugins.iter().flatten().try_for_each(|p| plugin_id(p))?;
    blocking(move |ctx| {
        sync::start(
            ctx,
            folders.as_deref(),
            apps.as_deref(),
            plugins.as_deref(),
            &steps(on_step),
        )
    })
    .await
    .map_err(Problem::from)
}

/// Returns where this computer's previous settings were backed up. `skip_folders`: the ones
/// this computer keeps its own version of; `skip_apps`, `skip_plugins`: the synced ones it
/// leaves out.
#[tauri::command]
async fn join(
    skip_folders: Option<Vec<String>>,
    skip_apps: Option<Vec<String>>,
    skip_plugins: Option<Vec<String>>,
    on_step: Channel<String>,
) -> Result<String> {
    let keep = known_folders(skip_folders.unwrap_or_default())?;
    let (skip_apps, skip_plugins) = (
        skip_apps.unwrap_or_default(),
        skip_plugins.unwrap_or_default(),
    );
    skip_apps.iter().try_for_each(|a| app_name(a))?;
    skip_plugins.iter().try_for_each(|p| plugin_id(p))?;
    blocking(move |ctx| {
        let login = sync::run(None, "gh", &["api", "user", "--jq", ".login"])?;
        sync::join(
            ctx,
            &login,
            &keep,
            &skip_apps,
            &skip_plugins,
            &steps(on_step),
        )
        .map(|p| p.display().to_string())
    })
    .await
    .map_err(Problem::from)
}

/// Puts a safety backup back; `name` must be a plain backup file name, never a path.
#[tauri::command]
async fn restore_backup(name: String) -> Result<()> {
    if !sync::is_backup_name(&name) {
        return Err("not a backup".to_string().into());
    }
    blocking(move |ctx| sync::restore_backup(ctx, &name))
        .await
        .map_err(Problem::from)
}

/// Installs exactly the synced apps this computer is missing, or those of them in `only`;
/// nothing else the page names. Returns how the snapshot of the system before it went.
#[tauri::command]
async fn install_apps(only: Option<Vec<String>>) -> Result<SnapshotOutcome> {
    only.iter().flatten().try_for_each(|a| app_name(a))?;
    blocking(move |ctx| {
        let mut missing = sync::missing_apps(
            &ctx.load_data(),
            &ctx.host,
            &sync::installed(),
            &sync::repos_here(),
        );
        if let Some(only) = &only {
            missing.retain(|a| only.contains(a));
        }
        if missing.is_empty() {
            Ok(SnapshotOutcome::Unavailable)
        } else {
            sync::install_apps(&missing)
        }
    })
    .await
    .map_err(Problem::from)
}

/// Adds exactly the synced plugins this computer is waiting for, or those of them in `only`;
/// nothing else the page names.
#[tauri::command]
async fn install_plugins(only: Option<Vec<String>>) -> Result<Data> {
    only.iter().flatten().try_for_each(|p| plugin_id(p))?;
    blocking(move |ctx| {
        let mut ids = ctx.load_data().missing_plugins(&ctx.host);
        if let Some(only) = &only {
            ids.retain(|p| only.contains(p));
        }
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

fn app_name(app: &str) -> Result<()> {
    if sync::is_package_name(app) {
        Ok(())
    } else {
        Err(format!("not an app: {app}").into())
    }
}

/// Whether one computer gets a plugin (see `Data::place`), or with no computer every one.
#[tauri::command]
async fn set_plugin(computer: Option<String>, plugin: String, on: bool) -> Result<Data> {
    plugin_id(&plugin)?;
    blocking(move |ctx| sync::set_plugin(ctx, computer.as_deref(), &plugin, on))
        .await
        .map_err(Problem::from)
}

/// Leaves a synced plugin out on this computer only, or takes it back.
#[tauri::command]
async fn plugin_here(plugin: String, on: bool) -> Result<Data> {
    plugin_id(&plugin)?;
    edit(move |d, host| d.set_plugin_here(host, &plugin, on)).await
}

/// Switches a plugin on or off in the Omarchy shell on this computer.
#[tauri::command]
async fn set_plugin_enabled(id: String, on: bool) -> Result<()> {
    plugin_id(&id)?;
    blocking(move |ctx| sync::set_plugin_enabled(ctx, &id, on))
        .await
        .map_err(Problem::from)
}

/// Adds a plugin from its git address (after the person read the warning). Only a plain https
/// or ssh address reaches the command.
#[tauri::command]
async fn add_plugin(url: String) -> Result<()> {
    let url = url.trim().to_string();
    if !sync::is_git_address(&url) {
        return Err(
            "A plugin's address starts with https:// or git@ and has no spaces."
                .to_string()
                .into(),
        );
    }
    blocking(move |ctx| sync::add_plugin(ctx, &url))
        .await
        .map_err(Problem::from)
}

/// Updates one plugin from git, or all of them, a step each. Returns a summary to show.
#[tauri::command]
async fn update_plugins(
    id: Option<String>,
    on_step: Channel<String>,
) -> Result<sync::PluginsUpdated> {
    if let Some(id) = &id {
        plugin_id(id)?;
    }
    blocking(move |ctx| sync::update_plugins(ctx, id.as_deref(), &steps(on_step)))
        .await
        .map_err(Problem::from)
}

/// Removes a plugin here and, at their next sync, from the computers that sync it (after the
/// person confirmed).
#[tauri::command]
async fn remove_plugin(id: String) -> Result<()> {
    plugin_id(&id)?;
    blocking(move |ctx| sync::remove_plugin(ctx, &id))
        .await
        .map_err(Problem::from)
}

/// Adds an extra package repository on this computer, as another computer has it (after the
/// person confirmed); only one this computer needs and lacks, by its recorded name.
#[tauri::command]
async fn add_repository(name: String) -> Result<SnapshotOutcome> {
    blocking(move |ctx| {
        let data = ctx.load_data();
        let needed = sync::missing_repos(&data, &ctx.host, &sync::repos_here());
        if !needed.iter().any(|r| r.name == name) {
            return Err(format!("{name} isn't a repository this computer needs"));
        }
        sync::add_repository(&data, &name)
    })
    .await
    .map_err(Problem::from)
}

/// Which pieces of the macOS theme are installed; none before it has been downloaded.
#[tauri::command]
async fn macos_theme_status() -> Result<Option<macos_theme::Status>> {
    blocking(|_| macos_theme::status())
        .await
        .map_err(Problem::from)
}

/// Installs and removes pieces of the macOS theme and changes its settings, only pieces and
/// values `install.sh` knows. Returns what the script printed.
#[tauri::command]
async fn macos_theme_apply(
    add: Vec<String>,
    remove: Vec<String>,
    set: Vec<(String, f64)>,
    switch_to: Option<String>,
    on_step: Channel<String>,
) -> Result<String> {
    if let Some(p) = add
        .iter()
        .chain(&remove)
        .find(|p| !macos_theme::PIECES.contains(&p.as_str()))
    {
        return Err(format!("{p} isn't a piece of the macOS theme").into());
    }
    if let Some((name, value)) = set.iter().find(|(name, value)| {
        !macos_theme::SETTINGS.iter().any(|(n, low, high, whole)| {
            n == name && (low..=high).contains(&value) && (!whole || value.fract() == 0.0)
        })
    }) {
        return Err(format!("{name} can't be set to {value}").into());
    }
    if let Some(theme) = &switch_to {
        let others = blocking(|_| macos_theme::other_themes())
            .await
            .map_err(Problem::from)?;
        if !others.contains(theme) {
            return Err(format!("Omarchy has no theme called {theme}").into());
        }
    }
    blocking(move |_| {
        macos_theme::apply(&add, &remove, &set, switch_to.as_deref(), &steps(on_step))
    })
    .await
    .map_err(theme_failed)
}

/// The themes to switch to before removing the macOS theme.
#[tauri::command]
async fn macos_theme_others() -> Result<Vec<String>> {
    blocking(|_| macos_theme::other_themes())
        .await
        .map_err(Problem::from)
}

/// Gets the latest macOS theme and installs the installed pieces again from it.
#[tauri::command]
async fn macos_theme_update(on_step: Channel<String>) -> Result<String> {
    blocking(move |_| macos_theme::update(&steps(on_step)))
        .await
        .map_err(theme_failed)
}

/// The script explains itself in its last line; the whole output goes in the details.
fn theme_failed(log: String) -> Problem {
    let last = log
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    Problem::Failed {
        message: if last.is_empty() {
            sync::friendly(&log)
        } else {
            last.trim().to_string()
        },
        details: log,
    }
}

/// Shows the sync indicator on the Omarchy bar, or takes it off.
#[tauri::command]
async fn set_bar_indicator(on: bool) -> Result<()> {
    // Remembered per computer, so the app doesn't put it back.
    edit(move |d, host| d.computers.entry(host.into()).or_default().no_bar_indicator = !on).await?;
    blocking(move |ctx| sync::set_bar_indicator(ctx, on))
        .await
        .map_err(Problem::from)
}

/// What changed in the choices since they were last shared, in sentences; none when nothing.
#[tauri::command]
fn choices_changed() -> Vec<String> {
    Ctx::load().choice_changes()
}

/// Loads the shared data, applies one change, saves it, and hands the result back to the page.
/// Waits for a running sync first, so it can't be saved over (see `sync::edit`).
async fn edit(change: impl FnOnce(&mut Data, &str) + Send + 'static) -> Result<Data> {
    blocking(move |ctx| sync::edit(ctx, change))
        .await
        .map_err(Problem::from)
}

/// Whether one computer gets an app (see `Data::place`), or with no computer every one.
#[tauri::command]
async fn set_app(computer: Option<String>, app: String, on: bool) -> Result<Data> {
    app_name(&app)?;
    let ctx = Ctx::load();
    // An app from outside the repos syncs only through its install script.
    if on && ctx.load_data().outside.get(&app) == Some(&None) {
        return Err(format!(
            "{app} isn't in Arch's repos or the AUR, so add an install script before syncing it."
        )
        .into());
    }
    let installed_here = sync::installed().contains(&app);
    edit(move |d, host| match &computer {
        Some(c) => d.place_app(host, installed_here, c, &app, on),
        None => d.set_app_all(host, &app, on),
    })
    .await
}

/// Leaves a synced app out on this computer only, or takes it back.
#[tauri::command]
async fn app_here(app: String, on: bool) -> Result<Data> {
    app_name(&app)?;
    edit(move |d, host| d.set_app_here(host, &app, on)).await
}

/// Saves how to install an app from outside Arch's repos and the AUR: one command, run in an
/// empty folder in a terminal window.
#[tauri::command]
async fn set_app_script(app: String, command: String) -> Result<Data> {
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
    edit(move |d, _| {
        d.outside.insert(
            app,
            Some(sync::Script {
                command,
                ..Default::default()
            }),
        );
    })
    .await
}

/// Forgets an app's install script; it then stays on the computers that have it.
#[tauri::command]
async fn remove_app_script(app: String) -> Result<Data> {
    edit(move |d, _| {
        if let Some(script) = d.outside.get_mut(&app) {
            *script = None;
        }
    })
    .await
}

/// Installs one waiting app with its script, in a terminal window; only an app this computer
/// is really waiting for, with the script saved for it (nothing the page names itself).
#[tauri::command]
async fn install_with_script(app: String, on_step: Channel<String>) -> Result<SnapshotOutcome> {
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
    blocking(move |ctx| sync::set_folder(ctx, path, on))
        .await
        .map_err(Problem::from)
}

/// Tries the newer Omarchy's settings on this paused computer; the sync that follows saves its
/// own first (see `sync::settle_pause`).
#[tauri::command]
async fn try_omarchy() -> Result<Data> {
    edit(|d, host| {
        if d.should_pause(host)
            && let Some(newest) = d.newest_omarchy()
        {
            d.computers.entry(host.into()).or_default().tried = newest;
        }
    })
    .await
}

#[tauri::command]
async fn put_back_omarchy() -> Result<()> {
    let ctx = Ctx::load();
    if ctx
        .load_data()
        .computers
        .get(&ctx.host)
        .is_none_or(|c| c.tried == 0)
    {
        return Err("This computer isn't trying another Omarchy's settings."
            .to_string()
            .into());
    }
    Ok(blocking(sync::put_back_omarchy).await?)
}

/// Omarchy's own updater, in a terminal where it can ask for the password.
#[tauri::command]
fn update_omarchy() -> Result<()> {
    std::process::Command::new("xdg-terminal-exec")
        .args(["omarchy", "update"])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("xdg-terminal-exec: {e}").into())
}

#[tauri::command]
async fn folder_here(path: String, on: bool) -> Result<Data> {
    let path = known_folder(&path)?;
    edit(move |d, host| d.set_folder_here(host, path, on)).await
}

#[tauri::command]
async fn set_auto(on: bool) -> Result<Data> {
    let data =
        edit(move |d, host| d.computers.entry(host.into()).or_default().manual = !on).await?;
    blocking(move |ctx| sync::schedule(ctx, on)).await?;
    Ok(data)
}

#[tauri::command]
async fn set_install_auto(on: bool) -> Result<Data> {
    edit(move |d, host| {
        d.computers
            .entry(host.into())
            .or_default()
            .install_without_asking = on
    })
    .await
}

/// Each computer names itself; the name is shared with the others on the next sync.
#[tauri::command]
async fn rename_computer(name: String) -> Result<Data> {
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
    edit(move |d, host| d.computers.entry(host.into()).or_default().name = name).await
}

/// Takes a retired computer off the list. If it ever syncs again it simply reappears.
#[tauri::command]
async fn forget_computer(id: String) -> Result<Data> {
    let ctx = Ctx::load();
    if id == ctx.host || !ctx.load_data().computers.contains_key(&id) {
        return Err(format!("not another known computer: {id}").into());
    }
    edit(move |d, _| {
        d.computers.remove(&id);
    })
    .await
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

/// Whether a newer Omarchy Sync is released. Asks GitHub; nothing is downloaded.
#[tauri::command]
async fn check_update() -> Result<Option<update::Update>> {
    blocking(|_| update::check()).await.map_err(Problem::from)
}

/// Installs the latest release, which must be the version the page saw (nothing else can be
/// named), then restarts the app: the installed copy starts a moment later, so the one-window
/// check doesn't hand it to this closing window. A running sync finishes before the install.
#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    version: String,
    on_step: Channel<String>,
) -> Result<()> {
    blocking(move |ctx| {
        let step = steps(on_step);
        let latest = update::check()?
            .filter(|u| u.version == version)
            .ok_or_else(|| format!("{version} isn't the latest release"))?;
        update::install(ctx, &latest.version, &step)?;
        step("Restarting Omarchy Sync");
        // Only once this process is gone: a copy started while this window is still closing
        // would find it, hand itself over to it and exit, leaving nothing open. After 30
        // seconds it starts anyway; an old window still open then merely gets focused.
        std::process::Command::new("sh")
            .args([
                "-c",
                "i=0; while kill -0 \"$1\" 2>/dev/null && [ $((i += 1)) -lt 150 ]; do sleep 0.2; done; exec \"$0\"",
                &update::exe_path(),
                &std::process::id().to_string(),
            ])
            .spawn()
            .map_err(|e| e.to_string())?;
        app.exit(0);
        Ok::<(), String>(())
    })
    .await
    .map_err(Problem::from)
}

/// Opens a release's page on GitHub (what's new). Only a plain version number becomes an address.
#[tauri::command]
fn open_release(version: String) -> Result<()> {
    let Some((a, b, c)) = update::version(&version) else {
        return Err(format!("not a version: {version}").into());
    };
    std::process::Command::new("xdg-open")
        .arg(update::release_page(&format!("{a}.{b}.{c}")))
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string().into())
}

/// The web page of an app's package or a plugin's source: only a package name on Arch's or
/// the AUR's site, or a git address, becomes one. `info` is `pacman -Si` for an Arch package:
/// its repository makes it the package's own page (archlinux.org sends an `any` package on
/// from the x86_64 address), and without one it's the search.
fn page_url(kind: &str, name: &str, info: &str) -> Option<String> {
    match kind {
        "arch" if sync::is_package_name(name) => {
            let repo = info
                .lines()
                .find_map(|l| l.strip_prefix("Repository"))
                .map(|l| l.trim_start_matches([' ', ':']).trim())
                .filter(|r| !r.is_empty());
            Some(match repo {
                Some(repo) => format!("https://archlinux.org/packages/{repo}/x86_64/{name}/"),
                None => format!("https://archlinux.org/packages/?q={name}"),
            })
        }
        "aur" if sync::is_package_name(name) => {
            Some(format!("https://aur.archlinux.org/packages/{name}"))
        }
        // The app's own website, as its package names it (Omarchy's repository has no pages).
        // shortcut: https only, as is_git_address checks; an http:// site stays unlinked.
        "website" if sync::is_package_name(name) => info
            .lines()
            .find_map(|l| l.strip_prefix("URL"))
            .map(|l| l.trim_start_matches([' ', ':']).trim().to_string())
            .filter(|url| sync::is_git_address(url)),
        "plugin" if sync::is_git_address(name) => {
            // A browser opens neither git@host:path nor .git: the same page, as https.
            let path = name.trim_start_matches("https://").trim_end_matches(".git");
            let path = match path.strip_prefix("git@") {
                Some(ssh) => ssh.replacen(':', "/", 1),
                None => path.to_string(),
            };
            Some(format!("https://{path}"))
        }
        _ => None,
    }
}

#[tauri::command]
fn open_page(kind: String, name: String) -> Result<()> {
    let info = if (kind == "arch" || kind == "website") && sync::is_package_name(&name) {
        sync::run(None, "pacman", &["-Si", &name]).unwrap_or_default()
    } else {
        String::new()
    };
    let url = page_url(&kind, &name, &info)
        .ok_or_else(|| format!("nothing to open for {kind} {name}"))?;
    std::process::Command::new("xdg-open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string().into())
}

/// Where problems and ideas are reported.
fn issues() -> String {
    format!("https://github.com/{}/issues/new", update::GITHUB_REPO)
}

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
    let issues = issues();
    let url = match message {
        None => format!("{issues}/choose"),
        Some(message) => {
            let message = clean(&message, 500);
            let title: String = message.chars().take(100).collect();
            format!(
                "{issues}?template=bug.yml&title={}&what-happened={}&details={}&version={}",
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

#[cfg(test)]
mod tests {
    use super::page_url;

    #[test]
    fn only_known_pages_open() {
        let info = "Repository      : extra\nName            : yazi\n";
        assert_eq!(
            page_url("arch", "yazi", info).as_deref(),
            Some("https://archlinux.org/packages/extra/x86_64/yazi/")
        );
        assert_eq!(
            page_url("arch", "yazi", "").as_deref(),
            Some("https://archlinux.org/packages/?q=yazi")
        );
        assert_eq!(
            page_url("aur", "spotify", "").as_deref(),
            Some("https://aur.archlinux.org/packages/spotify")
        );
        assert_eq!(
            page_url("plugin", "https://github.com/acme/weather.git", "").as_deref(),
            Some("https://github.com/acme/weather")
        );
        assert_eq!(
            page_url("plugin", "git@github.com:acme/weather.git", "").as_deref(),
            Some("https://github.com/acme/weather")
        );
        let info = "Name            : brave-bin\nURL             : https://brave.com\n";
        assert_eq!(
            page_url("website", "brave-bin", info).as_deref(),
            Some("https://brave.com")
        );
        assert_eq!(page_url("website", "brave-bin", ""), None);
        assert_eq!(page_url("website", "x", "URL : file:///etc/passwd\n"), None);
        assert_eq!(page_url("arch", "yazi; rm -rf /", ""), None);
        assert_eq!(page_url("plugin", "file:///etc/passwd", ""), None);
        assert_eq!(page_url("omarchy", "yazi", ""), None);
    }
}
