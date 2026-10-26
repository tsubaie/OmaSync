//! The sync indicator on the Omarchy bar: a small shell plugin kept with the app, and the status it reads.

use super::*;

/// What the bar indicator shows (packaging/bar-plugin): `state` is synced, syncing, attention
/// or error, and `text` says it in a sentence. Best effort: a failed write only leaves the
/// indicator a step behind.
pub fn write_status(ctx: &Ctx, state: &str, text: &str) {
    let path = state_file(ctx).with_file_name("status.json");
    let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));
    let json = serde_json::json!({ "state": state, "text": text });
    // Written whole and then renamed into place, so the bar never reads half a file.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, json.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// The bar indicator: a small Omarchy shell plugin, kept with the app. The shell keeps a
/// widget's code by file name, so the widget's file name carries a number that goes up
/// whenever it changes (Indicator2, Indicator3: QML only loads names that are valid types,
/// with no hyphens); an older file is replaced, not edited.
pub const BAR_PLUGIN: &str = "io.github.tsubaie.omarchy-sync";

const BAR_FILES: &[(&str, &str)] = &[
    (
        "manifest.json",
        include_str!("../../../packaging/bar-plugin/manifest.json"),
    ),
    (
        "Indicator2.qml",
        include_str!("../../../packaging/bar-plugin/Indicator2.qml"),
    ),
];

pub fn bar_indicator(ctx: &Ctx) -> bool {
    ctx.home
        .join(PLUGIN_DIR)
        .join(BAR_PLUGIN)
        .join("manifest.json")
        .exists()
}

/// Puts the indicator on the bar unless the person took it off this computer, and brings an
/// installed one up to this version. Cheap when it's already current.
pub fn ensure_bar_indicator(ctx: &Ctx, data: &Data) -> Result<(), String> {
    let off = data
        .computers
        .get(&ctx.host)
        .is_some_and(|c| c.no_bar_indicator);
    let dir = ctx.home.join(PLUGIN_DIR).join(BAR_PLUGIN);
    let current = BAR_FILES
        .iter()
        .all(|(name, text)| std::fs::read_to_string(dir.join(name)).is_ok_and(|t| t == *text));
    let desktop = here();
    if off || current || !desktop.profile().supports_bar_indicator || !desktop.has_shell() {
        return Ok(());
    }
    set_bar_indicator(ctx, true)
}

/// Puts the indicator on the bar, or takes it off, through Omarchy's own plugin commands.
pub fn set_bar_indicator(ctx: &Ctx, on: bool) -> Result<(), String> {
    let desktop = here();
    let dir = ctx.home.join(PLUGIN_DIR).join(BAR_PLUGIN);
    if on {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // Older widget files go, so only this version's is left.
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if !BAR_FILES.iter().any(|(n, _)| *n == name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
        for (name, text) in BAR_FILES {
            std::fs::write(dir.join(name), text).map_err(|e| e.to_string())?;
        }
        desktop.shell(&["shell", "rescanPlugins"])?;
        // The shell scans in the background, so the new plugin may take a moment to be known.
        let mut enabled = Err(String::new());
        for _ in 0..20 {
            enabled = desktop.plugin(&["enable", BAR_PLUGIN]);
            if enabled.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        enabled?;
        // The shell caches each plugin folder's files from when it first read them, so a new
        // or changed widget file only loads after a restart (about a second; the bar redraws).
        desktop.restart_shell()
    } else {
        // Already off (or never on) is fine: the goal is that it's gone.
        let _ = desktop.plugin(&["disable", BAR_PLUGIN]);
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        let _ = desktop.shell(&["shell", "rescanPlugins"]);
        Ok(())
    }
}
