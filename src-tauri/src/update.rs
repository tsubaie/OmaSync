//! Updating Omarchy Sync itself: a newer release on GitHub, installed the way this copy was
//! installed. A source build isn't touched; it updates with git.

use crate::sync::{self, Ctx, run};
use serde::Serialize;
use std::path::Path;

/// Where Omarchy Sync lives on GitHub: releases, issues and the build records checked here.
pub const GITHUB_REPO: &str = "tsubaie/OmaSync";

/// How this copy was installed, which decides how it updates.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
pub enum How {
    /// The release package (what install.sh installs): the next release's package replaces it.
    Package,
    /// The AUR package omarchy-sync-bin: yay updates it.
    Aur,
    /// Built from source, or not owned by any package.
    Source,
}

/// This binary's path. Once a package has replaced the file, Linux reports the running copy
/// as "… (deleted)"; the path is what pacman and the relaunch need.
pub fn exe_path() -> String {
    let exe = std::env::current_exe().unwrap_or_default();
    exe.to_string_lossy()
        .trim_end_matches(" (deleted)")
        .to_string()
}

pub fn installed_how() -> How {
    parse_owner(&run(None, "pacman", &["-Qo", &exe_path()]).unwrap_or_default())
}

/// Reads `pacman -Qo`: "/usr/bin/omarchy-sync is owned by omarchy-sync 0.1.1-1".
pub fn parse_owner(out: &str) -> How {
    let owner = out
        .rsplit(" is owned by ")
        .next()
        .and_then(|s| s.split_whitespace().next());
    match owner {
        Some("omarchy-sync") => How::Package,
        Some("omarchy-sync-bin") => How::Aur,
        _ => How::Source,
    }
}

/// A newer release.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Update {
    pub version: String,
}

/// "0.1.2" or "v0.1.2" as numbers; anything else (a pre-release, a dev build) is none.
pub fn version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.parse::<u32>().ok());
    let v = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(v)
}

/// The version `tag` names, when it's newer than `current`.
pub fn newer(current: &str, tag: &str) -> Option<String> {
    let (now, then) = (version(current)?, version(tag)?);
    (then > now).then(|| format!("{}.{}.{}", then.0, then.1, then.2))
}

/// The page of a release, for the browser.
pub fn release_page(version: &str) -> String {
    format!("https://github.com/{GITHUB_REPO}/releases/tag/v{version}")
}

/// Asks GitHub for the latest release: the update when it's newer than this copy. Nothing is
/// downloaded.
pub fn check() -> Result<Option<Update>, String> {
    let url = format!("https://api.github.com/repos/{GITHUB_REPO}/releases/latest");
    let body = run(None, "curl", &["-fsS", "--max-time", "15", &url])?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let tag = json["tag_name"].as_str().unwrap_or_default();
    Ok(newer(env!("CARGO_PKG_VERSION"), tag).map(|version| Update { version }))
}

/// The line of a SHA256SUMS text that names `file`.
pub fn checksum_line(sums: &str, file: &str) -> Option<String> {
    sums.lines()
        .find(|l| l.split_whitespace().nth(1) == Some(file))
        .map(String::from)
}

/// Installs release `version` the way this copy was installed. A running sync finishes first,
/// and the sync lock is held so none starts while the binary changes. From the release
/// package: nothing installs unless the package's checksum matches the release's SHA256SUMS
/// and GitHub attests that the release workflow built it. The caller restarts the app.
pub fn install(ctx: &Ctx, version: &str, step: &dyn Fn(&str)) -> Result<(), String> {
    let _lock = match sync::lock(ctx) {
        Ok(l) => l,
        Err(_) => {
            step("Waiting for the sync already running");
            sync::lock_wait(ctx)?
        }
    };
    let how = installed_how();
    if how == How::Source {
        return Err(
            "installed from source: update with git pull and the build steps in the README".into(),
        );
    }
    if how == How::Aur {
        sync::plan(step, &["Installing", "Restarting Omarchy Sync"]);
        step("Installing");
        return sync::install_apps(&["omarchy-sync-bin".to_string()]);
    }
    let downloading = format!("Downloading Omarchy Sync {version}");
    sync::plan(
        step,
        &[
            &downloading,
            "Checking it's genuine",
            "Installing",
            "Restarting Omarchy Sync",
        ],
    );
    let dir = std::env::temp_dir().join(format!("omarchy-sync-update-{}", std::process::id()));
    let result = download_and_install(&dir, version, step);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn download_and_install(dir: &Path, version: &str, step: &dyn Fn(&str)) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let base = format!("https://github.com/{GITHUB_REPO}/releases/download/v{version}");
    let file = format!("omarchy-sync-{version}-1-x86_64.pkg.tar.zst");
    let path = dir.join(&file);
    let package = path.to_string_lossy();
    step(&format!("Downloading Omarchy Sync {version}"));
    // The package is the long download (megabytes), so its percentage shows on the page.
    sync::run_progress(
        Some(dir),
        "curl",
        &[
            "-fSL",
            "-#",
            "--max-time",
            "300",
            "-o",
            &file,
            &format!("{base}/{file}"),
        ],
        sync::curl_percent,
        step,
    )?;
    run(
        Some(dir),
        "curl",
        &[
            "-fsSL",
            "--max-time",
            "300",
            "-o",
            "SHA256SUMS",
            &format!("{base}/SHA256SUMS"),
        ],
    )?;
    step("Checking it's genuine");
    let sums = std::fs::read_to_string(dir.join("SHA256SUMS")).map_err(|e| e.to_string())?;
    let line = checksum_line(&sums, &file)
        .ok_or_else(|| format!("checksum: {file} isn't listed in the release's SHA256SUMS"))?;
    std::fs::write(dir.join("expected"), format!("{line}\n")).map_err(|e| e.to_string())?;
    run(Some(dir), "sha256sum", &["-c", "--quiet", "expected"])
        .map_err(|e| format!("checksum: {e}"))?;
    run(
        None,
        "gh",
        &["attestation", "verify", &package, "--repo", GITHUB_REPO],
    )
    .map_err(|e| format!("attestation: {e}"))?;
    step("Installing");
    run(None, "pkexec", &["pacman", "-U", "--noconfirm", &package]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(newer("0.1.1", "v0.1.2"), Some("0.1.2".into()));
        assert_eq!(newer("0.1.9", "v0.1.10"), Some("0.1.10".into()));
        assert_eq!(newer("0.1.1", "v0.1.1"), None);
        assert_eq!(newer("0.2.0", "v0.1.9"), None);
        assert_eq!(newer("0.1.1", "v1.0"), None);
        assert_eq!(newer("0.1.1", "v0.1.2-rc1"), None);
        assert_eq!(newer("0.1.1", ""), None);
    }

    #[test]
    fn the_owning_package_decides_how_to_update() {
        assert_eq!(
            parse_owner("/usr/bin/omarchy-sync is owned by omarchy-sync 0.1.1-1"),
            How::Package
        );
        assert_eq!(
            parse_owner("/usr/bin/omarchy-sync is owned by omarchy-sync-bin 0.1.2-1"),
            How::Aur
        );
        assert_eq!(parse_owner(""), How::Source);
        assert_eq!(
            parse_owner("error: No package owns /home/me/.local/bin/omarchy-sync"),
            How::Source
        );
    }

    #[test]
    fn the_checksum_line_is_picked_by_file_name() {
        let sums = "aaa  omarchy-sync-0.1.2-1-x86_64.pkg.tar.zst\nbbb  omarchy-sync-0.1.2-x86_64-linux.tar.gz\n";
        assert_eq!(
            checksum_line(sums, "omarchy-sync-0.1.2-1-x86_64.pkg.tar.zst").as_deref(),
            Some("aaa  omarchy-sync-0.1.2-1-x86_64.pkg.tar.zst")
        );
        assert_eq!(
            checksum_line(sums, "omarchy-sync-0.1.3-1-x86_64.pkg.tar.zst"),
            None
        );
    }
}
