//! The macOS theme for Omarchy: a copy of its repository kept in the data folder, driven
//! through the repository's own `install.sh` (`--add`, `--remove`, `--status --json`).
//! Nothing here re-implements the script; it only runs it and passes its lines on.

use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

const REPOSITORY: &str = "https://github.com/tsubaie/omarchy-macos-theme.git";

/// The pieces `install.sh` knows, in the order the page lists them.
pub const PIECES: [&str; 9] = [
    "theme",
    "rows",
    "icons",
    "wide-menu",
    "dock",
    "switcher",
    "menubar",
    "osd",
    "dictation-hud",
];

/// The settings `install.sh --set` takes, with the lowest and highest value of each.
pub const SETTINGS: [(&str, f64, f64); 2] = [("glass", 0.5, 1.0), ("magnification", 1.0, 2.0)];

/// What `install.sh --status --json` reports.
#[derive(Deserialize, serde::Serialize, Debug, PartialEq, Eq)]
pub struct Status {
    /// The macOS theme is the current Omarchy theme.
    pub active: bool,
    /// Each piece and whether it is installed.
    pub features: serde_json::Map<String, serde_json::Value>,
    /// Each setting and its value; empty from a copy of the script older than the settings.
    #[serde(default)]
    pub settings: serde_json::Map<String, serde_json::Value>,
}

/// Where the copy of the repository lives. `OMARCHY_MACOS_THEME` points at another copy, such
/// as a checkout being worked on.
fn folder() -> PathBuf {
    if let Some(dir) = std::env::var_os("OMARCHY_MACOS_THEME") {
        return dir.into();
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    data.join("omarchy-macos-theme")
}

/// The theme's state here, or none before its repository has been downloaded.
pub fn status() -> Result<Option<Status>, String> {
    let script = folder().join("install.sh");
    if !script.exists() {
        return Ok(None);
    }
    let out = Command::new("bash")
        .arg(&script)
        .args(["--status", "--json"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    serde_json::from_slice(&out.stdout)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// Installs and removes the given pieces and sets the given settings (already checked against
/// `PIECES` and `SETTINGS`), downloading the repository first if needed. Returns everything the
/// script printed.
pub fn apply(
    add: &[String],
    remove: &[String],
    set: &[(String, f64)],
    step: &dyn Fn(&str),
) -> Result<String, String> {
    if !folder().join("install.sh").exists() {
        step("Downloading the macOS theme");
        let mut git = Command::new("git");
        git.args(["clone", "-q", "--depth", "1", REPOSITORY])
            .arg(folder());
        run(git, step)?;
    }
    let mut args = Vec::new();
    for piece in remove {
        args.extend(["--remove".to_string(), piece.clone()]);
    }
    for piece in add {
        args.extend(["--add".to_string(), piece.clone()]);
    }
    for (name, value) in set {
        args.extend(["--set".to_string(), format!("{name}={value}")]);
    }
    let mut script = Command::new("bash");
    script.arg(folder().join("install.sh")).args(args);
    run(script, step)
}

/// Gets the latest version of the theme, then installs every installed piece again from it.
pub fn update(step: &dyn Fn(&str)) -> Result<String, String> {
    step("Getting the latest version");
    let mut git = Command::new("git");
    git.arg("-C")
        .arg(folder())
        .args(["pull", "-q", "--ff-only"]);
    let mut log = run(git, step)?;
    let installed: Vec<String> = status()?
        .ok_or("The macOS theme isn't downloaded yet.")?
        .features
        .into_iter()
        .filter(|(_, on)| on == &serde_json::Value::Bool(true))
        .map(|(piece, _)| piece)
        .collect();
    log += &apply(&installed, &[], &[], step)?;
    Ok(log)
}

/// Runs a command, passing each `==>` line on as a step; returns all of its output, or that
/// output as the error when it fails.
fn run(mut cmd: Command, step: &dyn Fn(&str)) -> Result<String, String> {
    // One pipe for both streams keeps warnings next to the step they belong to.
    let (reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(writer.try_clone().map_err(|e| e.to_string())?)
        .stderr(writer)
        .spawn()
        .map_err(|e| e.to_string())?;
    // Dropping the command closes its copies of the write end, so reading ends when it exits.
    drop(cmd);
    let mut log = String::new();
    for line in BufReader::new(reader).lines() {
        let line = line
            .map_err(|e| e.to_string())?
            .replace("\x1b[1m", "")
            .replace("\x1b[0m", "");
        if let Some(said) = line.strip_prefix("==> ") {
            step(said);
        }
        log.push_str(&line);
        log.push('\n');
    }
    if child.wait().map_err(|e| e.to_string())?.success() {
        Ok(log)
    } else {
        Err(log)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_passes_steps_on_and_keeps_the_rest() {
        let steps = std::cell::RefCell::new(Vec::new());
        let mut sh = Command::new("sh");
        sh.args([
            "-c",
            "printf '\\033[1m==>\\033[0m One\\n  note\\n'; echo oops >&2",
        ]);
        let log = run(sh, &|s| steps.borrow_mut().push(s.to_string())).unwrap();
        assert_eq!(steps.into_inner(), ["One"]);
        assert_eq!(log, "==> One\n  note\noops\n");
        let mut sh = Command::new("sh");
        sh.args(["-c", "echo why >&2; exit 1"]);
        let failed = run(sh, &|_| ()).unwrap_err();
        assert_eq!(failed, "why\n");
    }
}
