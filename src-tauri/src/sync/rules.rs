//! What syncs: the folders on offer, the ones nested inside others, and chezmoi's ignore rules.

use super::DistroProfile;

/// Folders offered on the "What's synced" tab: (path under home, plain name, on by default).
pub const FOLDERS: &[(&str, &str, bool)] = &[
    (".config/omarchy", "Other Omarchy settings", true),
    // Inside another folder here, but each synced on its own (see NESTED).
    (".config/omarchy/themes", "Current theme", true),
    (".config/omarchy/extensions", "Omarchy menu additions", true),
    (".config/omarchy/hooks", "Hooks", true),
    (".config/omarchy/plugins", "Plugins", true),
    (
        ".config/omarchy/shell.json",
        "Bar layout and plugin settings",
        true,
    ),
    (".config/hypr", "Other window settings", true),
    (".config/hypr/bindings.lua", "Keyboard shortcuts", true),
    (
        ".config/hypr/input.lua",
        "Keyboard, mouse and touchpad",
        true,
    ),
    (".config/hypr/looknfeel.lua", "Look and feel", true),
    (
        ".config/hypr/autostart.lua",
        "Apps that start with Omarchy",
        true,
    ),
    (".config/hypr/hyprsunset.conf", "Night light", true),
    (
        ".local/share/applications",
        "Web apps and their icons",
        true,
    ),
    (".config/alacritty", "Alacritty terminal", true),
    (".config/ghostty", "Ghostty terminal", true),
    (".config/kitty", "Kitty terminal", true),
    (".config/foot", "Foot terminal", true),
    (".config/starship.toml", "Terminal prompt", true),
    (".config/nvim", "Neovim editor", false),
    (".config/btop", "System monitor", false),
    (".config/lazygit", "Lazygit", false),
    (".config/fastfetch", "System info screen", false),
    (".config/git", "Git settings", false),
];

/// Omarchy's and Hyprland's own settings (Hyprland's lock and idle settings live in its
/// folder), only offered where Omarchy runs. The rest of FOLDERS are apps' own settings in
/// their XDG folders, the same on any distribution.
pub const DESKTOP_FOLDERS: &[&str] = &[
    ".config/omarchy",
    ".config/hypr",
    ".config/waybar",
    ".config/walker",
    ".config/mako",
    ".config/swayosd",
    ".config/uwsm",
    ".XCompose",
];

pub fn is_desktop_folder(path: &str) -> bool {
    DESKTOP_FOLDERS
        .iter()
        .any(|d| path == *d || path.starts_with(&format!("{d}/")))
}

/// FOLDERS this desktop can use: all of them on Omarchy, the apps' own elsewhere.
pub fn folders_for_profile(profile: &DistroProfile) -> Vec<(&'static str, &'static str, bool)> {
    FOLDERS
        .iter()
        .copied()
        // Omarchy's own settings go where its themes do.
        .filter(|(p, _, _)| profile.supports_themes || !is_desktop_folder(p))
        .collect()
}

/// The folders of `folders_for_profile` that are on by default.
#[cfg(test)]
pub fn default_folders_for_profile(profile: &DistroProfile) -> Vec<&'static str> {
    folders_for_profile(profile)
        .into_iter()
        .filter(|(_, _, on)| *on)
        .map(|(p, _, _)| p)
        .collect()
}

/// The app whose settings an app-data folder holds, by package name. Omarchy ships settings for
/// several of these whether or not the app is installed, so a folder on its own says little.
pub fn folder_app(path: &str) -> Option<&'static str> {
    match path {
        ".config/alacritty" => Some("alacritty"),
        ".config/ghostty" => Some("ghostty"),
        ".config/kitty" => Some("kitty"),
        ".config/foot" => Some("foot"),
        ".config/starship.toml" => Some("starship"),
        ".config/nvim" => Some("neovim"),
        ".config/btop" => Some("btop"),
        ".config/lazygit" => Some("lazygit"),
        ".config/fastfetch" => Some("fastfetch"),
        ".config/git" => Some("git"),
        _ => None,
    }
}

/// The data file label shown when only the app list or sync choices changed.
pub const DATA_LABEL: &str = "Apps and sync choices";

/// Folders and files inside another synced folder; the ignore rules below leave them out when
/// they're off. (The ignore rules list them again; a test checks the two agree.)
pub const NESTED: &[&str] = &[
    ".config/omarchy/plugins",
    ".config/omarchy/shell.json",
    ".config/omarchy/themes",
    ".config/omarchy/extensions",
    ".config/omarchy/hooks",
    ".config/hypr/bindings.lua",
    ".config/hypr/input.lua",
    ".config/hypr/looknfeel.lua",
    ".config/hypr/autostart.lua",
    ".config/hypr/hyprsunset.conf",
];

// monitors.lua differs per screen, so it never leaves the computer it was made on.
pub(crate) const IGNORE: &str = r#"**/.git
**/.git/**
**/node_modules/**
**/*.bak.*
.config/hypr/monitors.lua
.config/hypr/monitors.conf
.config/omarchy/backgrounds
.config/omarchy/backgrounds/**
{{- range list ".config/omarchy/plugins" ".config/omarchy/shell.json" ".config/omarchy/themes" ".config/omarchy/extensions" ".config/omarchy/hooks" ".config/hypr/bindings.lua" ".config/hypr/input.lua" ".config/hypr/looknfeel.lua" ".config/hypr/autostart.lua" ".config/hypr/hyprsunset.conf" }}
{{- if not (has . $.folders) }}
{{ . }}
{{ . }}/**
{{- end }}
{{- end }}
{{- $c := get .computers (env "OMARCHY_SYNC_HOST" | default (include "/etc/machine-id" | trim | trunc 12)) | default dict }}
{{- $skipFiles := get $c "skip_files" | default list }}
{{- if get $c "paused" }}
{{- $skipFiles = concat $skipFiles (list ".config/omarchy" ".config/hypr") }}
{{- end }}
{{- range $skipFiles }}
{{ . }}
{{- end }}
{{- $mine := get $c "plugins" | default list }}
{{- $skip := get $c "skip_plugins" | default list }}
{{- $fromGit := get . "plugin_sources" | default dict }}
.config/omarchy/plugins/*
.config/omarchy/plugins/*/**
{{- /* Exceptions win over every other rule, so they're only made while the folder syncs here. */}}
{{- if and (has ".config/omarchy/plugins" .folders) (not (has ".config/omarchy" $skipFiles)) (not (has ".config/omarchy/plugins" $skipFiles)) }}
{{- range .plugins | default list }}
{{- /* A plugin from git is added from git on each computer, never copied. */}}
{{- if and (has . $mine) (not (has . $skip)) (not (hasKey $fromGit .)) }}
!.config/omarchy/plugins/{{ . }}
!.config/omarchy/plugins/{{ . }}/**
{{- end }}
{{- end }}
{{- end }}
.config/omarchy/themes/*
.config/omarchy/themes/*/**
{{- /* Only the current theme's own folder (a custom theme, or tweaks to a built-in one). */}}
{{- $theme := get . "theme" | default "" }}
{{- if and $theme (has ".config/omarchy/themes" .folders) (not (has ".config/omarchy" $skipFiles)) (not (has ".config/omarchy/themes" $skipFiles)) }}
!.config/omarchy/themes/{{ $theme }}
!.config/omarchy/themes/{{ $theme }}/**
{{- end }}
.local/share/applications/*
.local/share/icons/hicolor/256x256/apps/*
{{- if and (has ".local/share/applications" .folders) (not (has ".local/share/applications" $skipFiles)) }}
{{- range $id, $other := .computers }}
{{- range get $other "web_apps" | default list }}
!.local/share/applications/{{ . }}
{{- end }}
{{- range get $other "web_icons" | default list }}
!.local/share/icons/hicolor/256x256/apps/{{ . }}
{{- end }}
{{- end }}
{{- end }}
"#;

/// Web app shortcuts, and where omarchy-webapp-install keeps the icons it downloads for them.
/// Only web apps and their icons sync from these: not other shortcuts or caches.
pub(crate) const APPS_DIR: &str = ".local/share/applications";

pub(crate) const ICON_DIR: &str = ".local/share/icons/hicolor/256x256/apps";

/// Where Omarchy shell plugins live; each one is a folder named by its ID.
pub const PLUGIN_DIR: &str = ".config/omarchy/plugins";

/// Omarchy's own settings, which can change shape between its major versions.
pub const OMARCHY_OWN: [&str; 2] = [".config/omarchy", ".config/hypr"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::*;

    #[test]
    fn nested_folders_match_the_ignore_rules() {
        for f in NESTED {
            assert!(
                IGNORE.contains(&format!("\"{f}\"")),
                "{f} missing from IGNORE"
            );
            assert!(
                FOLDERS.iter().any(|(p, _, _)| p == f),
                "{f} missing from FOLDERS"
            );
        }
        let mut d = Data {
            folders: vec![".config/hypr".into()],
            version: 2,
            ..Default::default()
        };
        d.upgrade();
        assert!(d.folders.iter().any(|f| f == ".config/hypr/bindings.lua"));
        assert!(!d.folders.iter().any(|f| f == ".config/omarchy/themes"));
    }

    #[test]
    fn plain_desktops_get_only_the_apps_own_folders() {
        let all = folders_for_profile(&DistroProfile::OMARCHY);
        assert_eq!(all, FOLDERS);
        let plain = folders_for_profile(&DistroProfile::PLAIN);
        let paths: Vec<&str> = plain.iter().map(|(p, _, _)| *p).collect();
        for core in [
            ".config/ghostty",
            ".config/alacritty",
            ".config/kitty",
            ".config/nvim",
            ".config/git",
            ".config/lazygit",
            ".config/btop",
            ".config/fastfetch",
            ".config/starship.toml",
            ".local/share/applications",
        ] {
            assert!(paths.contains(&core), "{core} missing");
        }
        assert!(paths.iter().all(|p| !p.starts_with(".config/omarchy")));
        assert!(paths.iter().all(|p| !p.starts_with(".config/hypr")));
        assert!(is_desktop_folder(".config/hypr/bindings.lua"));
        assert!(!is_desktop_folder(".config/hyprx"));
        assert_eq!(all.len() - plain.len(), 12);
        // Defaults follow the catalog's own.
        let defaults = default_folders_for_profile(&DistroProfile::PLAIN);
        assert!(defaults.contains(&".config/ghostty") && !defaults.contains(&".config/nvim"));
        assert!(default_folders_for_profile(&DistroProfile::OMARCHY).contains(&".config/hypr"));
    }
}
