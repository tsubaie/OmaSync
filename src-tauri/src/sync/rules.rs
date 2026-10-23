//! What syncs: the folders on offer, the ones nested inside others, and chezmoi's ignore rules.

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
.config/omarchy/plugins/*
.config/omarchy/plugins/*/**
{{- /* Exceptions win over every other rule, so they're only made while the folder syncs here. */}}
{{- if and (has ".config/omarchy/plugins" .folders) (not (has ".config/omarchy" $skipFiles)) (not (has ".config/omarchy/plugins" $skipFiles)) }}
{{- range .plugins | default list }}
{{- if and (has . $mine) (not (has . $skip)) }}
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
}
