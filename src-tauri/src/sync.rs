//! Everything that touches chezmoi, git, gh and pacman. No UI code here.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const REPO: &str = "omarchy-sync";

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
const IGNORE: &str = r#"**/.git
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
const APPS_DIR: &str = ".local/share/applications";
const ICON_DIR: &str = ".local/share/icons/hicolor/256x256/apps";

/// Where Omarchy shell plugins live; each one is a folder named by its ID.
pub const PLUGIN_DIR: &str = ".config/omarchy/plugins";

/// Base system packages that are never offered as "apps".
const SYSTEM: &[&str] = &[
    "base",
    "linux",
    "linux-firmware",
    "sudo",
    "mkinitcpio",
    "efibootmgr",
    "intel-ucode",
    "amd-ucode",
];

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Data {
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// The file name of the current background, kept in the sync folder's `.background`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default)]
    pub computers: BTreeMap<String, Computer>,
    /// Plugins shared between computers; the rest stay on the computer that has them.
    #[serde(default)]
    pub plugins: Vec<String>,
    /// Apps that aren't in Arch's repos or the AUR, so `yay` can't install them, with the
    /// script someone added to install them (none yet: they can't sync).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outside: BTreeMap<String, Option<Script>>,
    /// Extra package repositories (beyond Omarchy's and Arch's) some computer has, by name, so a
    /// computer without one can add it before installing apps from it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub repositories: BTreeMap<String, Repository>,
    /// Which extra repository each synced app comes from. Such an app installs only from it,
    /// never from the AUR instead.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub app_repo: BTreeMap<String, String>,
    /// The layout of this file; older layouts are upgraded when read.
    #[serde(default)]
    pub version: u32,
}

/// An extra package repository, as the computer that has it uses it.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Repository {
    pub servers: Vec<String>,
    /// pacman's signature rules for it, word by word ("Required", "DatabaseTrustedOnly", …).
    #[serde(default)]
    pub sig_level: Vec<String>,
    /// The key that signs it, copied from that computer's pacman keyring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<RepoKey>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct RepoKey {
    /// 40 hex digits, to check against the vendor's website.
    pub fingerprint: String,
    /// Whose key it is, as the key says ("Codex Linux Repository").
    pub owner: String,
    /// The public key itself (ASCII-armored).
    pub armored: String,
}

/// A map changed on two sides of a merge: entries `mine` changed (added, altered or removed)
/// win; the rest come from `theirs`.
fn merge_map<T: Clone + PartialEq>(
    base: &BTreeMap<String, T>,
    theirs: &BTreeMap<String, T>,
    mine: &BTreeMap<String, T>,
) -> BTreeMap<String, T> {
    let mut out = theirs.clone();
    for key in base.keys().chain(mine.keys()) {
        if mine.get(key) != base.get(key) {
            match mine.get(key) {
                Some(v) => out.insert(key.clone(), v.clone()),
                None => out.remove(key),
            };
        }
    }
    out
}

/// Repositories every Omarchy computer has; only others are recorded and offered.
const STANDARD_REPOS: &[&str] = &[
    "omarchy",
    "core",
    "extra",
    "multilib",
    "core-testing",
    "extra-testing",
    "multilib-testing",
];

/// How to install an app from outside Arch's repos and the AUR: a command, run in an empty
/// folder in a terminal window the person can see (it may download a repository itself).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Script {
    pub command: String,
    /// Only read: an earlier version kept a repository to clone apart from the command.
    /// `Data::upgrade` folds it into the command.
    #[serde(default, skip_serializing)]
    pub repo: String,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct Computer {
    /// What people call this computer; it names itself, starting from its hostname.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Names this computer synced under before (hostnames from older versions), so its
    /// earlier history still resolves to it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aka: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_apps: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_files: Vec<String>,
    /// The plugins on this computer. A synced plugin it doesn't have stays off it (by the ignore
    /// rules) until it takes it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<String>,
    /// Synced plugins this computer doesn't want.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_plugins: Vec<String>,
    /// The web app shortcuts on this computer, and their icon files; every computer gets them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub web_apps: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub web_icons: Vec<String>,
    /// Turns off the background timer for this computer.
    #[serde(default, skip_serializing_if = "is_false")]
    pub manual: bool,
    /// Installs synced apps during a sync instead of waiting for approval.
    #[serde(default, skip_serializing_if = "is_false")]
    pub install_without_asking: bool,
    /// The person took the sync indicator off this computer's Omarchy bar (it's on by default).
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_bar_indicator: bool,
    /// The Omarchy version here, as `omarchy version` prints it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub omarchy: String,
    /// Omarchy's own settings don't sync here for now: a newer Omarchy runs on another computer,
    /// and settings made for it may not work on this one (see `should_pause`).
    #[serde(default, skip_serializing_if = "is_false")]
    pub paused: bool,
    /// The newer Omarchy major version whose settings the person chose to try here anyway.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub tried: u32,
}

fn is_false(b: &bool) -> bool {
    !b
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Omarchy's own settings, which can change shape between its major versions.
pub const OMARCHY_OWN: [&str; 2] = [".config/omarchy", ".config/hypr"];

/// The major version in what `omarchy version` prints ("4.0.6-1" is 4); none for a dev copy.
pub fn major(version: &str) -> Option<u32> {
    version.split('.').next()?.parse().ok()
}

impl Data {
    /// Older versions keyed computers by hostname, and Omarchy's default hostname is "omarchy"
    /// on every machine. Moves this computer's hostname entry to its machine ID, once.
    pub fn adopt_legacy(&mut self, id: &str, hostname: &str) {
        if hostname.is_empty() || hostname == id || self.computers.contains_key(id) {
            return;
        }
        if let Some(mut old) = self.computers.remove(hostname) {
            old.aka.push(hostname.to_string());
            if old.name.is_empty() {
                old.name = hostname.to_string();
            }
            self.computers.insert(id.to_string(), old);
        }
    }

    /// Version 1 split plugins and the bar's settings out of `.config/omarchy`; anyone who
    /// synced that folder keeps syncing both. Version 2 chose plugins one by one; every plugin a
    /// computer had (they all synced before) stays synced.
    pub fn upgrade(&mut self) {
        if self.version < 1 && self.folders.iter().any(|f| f == ".config/omarchy") {
            for f in &NESTED[..2] {
                toggle(&mut self.folders, f, true);
            }
        }
        if self.version < 2 {
            let all: BTreeSet<String> = self
                .computers
                .values()
                .flat_map(|c| c.plugins.iter().cloned())
                .collect();
            for p in all {
                toggle(&mut self.plugins, &p, true);
            }
        }
        // Version 3 split themes, menus, hooks and the window settings into their own switches;
        // each stays on wherever its folder synced.
        if self.version < 3 {
            for f in &NESTED[2..] {
                let parent = f.rsplit_once('/').map_or("", |(p, _)| p);
                if self.folders.iter().any(|x| x == parent) {
                    toggle(&mut self.folders, f, true);
                }
            }
        }
        // A script that kept its repository apart becomes one command that clones it first.
        for (app, script) in self.outside.iter_mut() {
            if let Some(s) = script.as_mut().filter(|s| !s.repo.is_empty()) {
                s.command = format!("git clone {} {app} && cd {app} && {}", s.repo, s.command);
                s.repo.clear();
            }
        }
        self.version = 3;
    }

    /// The computer ID behind a git author name: an ID, or an older name listed in `aka`.
    pub fn resolve(&self, author: &str) -> String {
        if self.computers.contains_key(author) {
            return author.to_string();
        }
        self.computers
            .iter()
            .find(|(_, c)| c.aka.iter().any(|a| a == author))
            .map_or(author.to_string(), |(id, _)| id.clone())
    }

    /// A name for a new computer: its hostname, made unique if another computer already uses it.
    pub fn fresh_name(&self, id: &str, hostname: &str) -> String {
        let taken = |n: &str| {
            self.computers
                .iter()
                .any(|(other, c)| other != id && c.name == n)
        };
        let base = if hostname.is_empty() {
            "Computer"
        } else {
            hostname
        };
        (1..)
            .map(|i| {
                if i == 1 {
                    base.to_string()
                } else {
                    format!("{base} {i}")
                }
            })
            .find(|n| !taken(n))
            .unwrap()
    }

    pub fn wants_app(&self, host: &str, app: &str) -> bool {
        self.apps.iter().any(|a| a == app)
            && !self
                .computers
                .get(host)
                .is_some_and(|c| c.skip_apps.iter().any(|a| a == app))
    }

    pub fn uses_folder(&self, host: &str, folder: &str) -> bool {
        let paused = |c: &Computer| {
            c.paused
                && OMARCHY_OWN
                    .iter()
                    .any(|o| folder == *o || folder.starts_with(&format!("{o}/")))
        };
        // Leaving a folder out leaves out what's synced separately inside it too (the ignore
        // rules do), or chezmoi would be asked to add files whose folder isn't synced here.
        let skipped = |c: &Computer| {
            c.skip_files
                .iter()
                .any(|f| folder == f || folder.starts_with(&format!("{f}/")))
        };
        !self
            .computers
            .get(host)
            .is_some_and(|c| skipped(c) || paused(c))
    }

    /// The newest Omarchy major version among the computers.
    pub fn newest_omarchy(&self) -> Option<u32> {
        self.computers
            .values()
            .filter_map(|c| major(&c.omarchy))
            .max()
    }

    /// Whether Omarchy's own settings should stop syncing here: another computer runs a newer
    /// major version (the shared settings follow the newest), and the person hasn't chosen to
    /// try its settings. Each computer only pauses itself; the newest ones carry on.
    pub fn should_pause(&self, host: &str) -> bool {
        let Some(c) = self.computers.get(host) else {
            return false;
        };
        match (major(&c.omarchy), self.newest_omarchy()) {
            (Some(mine), Some(newest)) => newest > mine && c.tried != newest,
            _ => false,
        }
    }

    #[cfg(test)]
    pub fn set_app(&mut self, host: &str, app: &str, on: bool) {
        toggle(
            &mut self.computers.entry(host.into()).or_default().skip_apps,
            app,
            !on,
        );
    }

    pub fn set_folder_here(&mut self, host: &str, folder: &str, on: bool) {
        toggle(
            &mut self.computers.entry(host.into()).or_default().skip_files,
            folder,
            !on,
        );
    }

    /// A three-way merge of two versions that both started from `base`: list changes from
    /// both sides are kept, and a computer's entry comes from the side that changed it
    /// (`mine` when both did).
    pub fn merge(base: &Data, theirs: &Data, mine: &Data) -> Data {
        let list = |b: &[String], t: &[String], m: &[String]| -> Vec<String> {
            let mut out: Vec<String> = t.to_vec();
            for x in m.iter().filter(|x| !b.contains(x)) {
                toggle(&mut out, x, true);
            }
            out.retain(|x| !(b.contains(x) && !m.contains(x)));
            out
        };
        let outside = merge_map(&base.outside, &theirs.outside, &mine.outside);
        let mut computers = theirs.computers.clone();
        for id in base.computers.keys().chain(mine.computers.keys()) {
            if mine.computers.get(id) != base.computers.get(id) {
                match mine.computers.get(id) {
                    Some(c) => computers.insert(id.clone(), c.clone()),
                    None => computers.remove(id),
                };
            }
        }
        Data {
            folders: list(&base.folders, &theirs.folders, &mine.folders),
            apps: list(&base.apps, &theirs.apps, &mine.apps),
            plugins: list(&base.plugins, &theirs.plugins, &mine.plugins),
            theme: if mine.theme != base.theme {
                mine.theme.clone()
            } else {
                theirs.theme.clone()
            },
            background: if mine.background != base.background {
                mine.background.clone()
            } else {
                theirs.background.clone()
            },
            computers,
            outside,
            repositories: merge_map(&base.repositories, &theirs.repositories, &mine.repositories),
            app_repo: merge_map(&base.app_repo, &theirs.app_repo, &mine.app_repo),
            version: theirs.version.max(mine.version),
        }
    }

    /// The same choices with every list in order, for comparing.
    pub fn sorted(mut self) -> Self {
        self.folders.sort();
        self.apps.sort();
        self.plugins.sort();
        for c in self.computers.values_mut() {
            for list in [
                &mut c.aka,
                &mut c.skip_apps,
                &mut c.skip_files,
                &mut c.plugins,
                &mut c.skip_plugins,
            ] {
                list.sort();
            }
        }
        // An entry left empty by switching something off and on again says nothing.
        self.computers.retain(|_, c| *c != Computer::default());
        self
    }

    pub fn wants_plugin(&self, host: &str, plugin: &str) -> bool {
        self.plugins.iter().any(|p| p == plugin)
            && self.uses_folder(host, PLUGIN_DIR)
            && !self
                .computers
                .get(host)
                .is_some_and(|c| c.skip_plugins.iter().any(|p| p == plugin))
    }

    #[cfg(test)]
    pub fn set_plugin(&mut self, host: &str, plugin: &str, on: bool) {
        toggle(
            &mut self.computers.entry(host.into()).or_default().skip_plugins,
            plugin,
            !on,
        );
    }

    /// Switches an app or plugin on or off for one computer. Switching it on while no computer
    /// has it on shares it with that computer and with `owners` (where it already is, so their
    /// copy is the one shared). Shared with only this computer still counts: computers added
    /// later get it. It stops being shared when every computer is switched off.
    fn place(
        &mut self,
        shared: fn(&mut Data) -> &mut Vec<String>,
        skip: fn(&mut Computer) -> &mut Vec<String>,
        owners: &[String],
        computer: &str,
        item: &str,
        on: bool,
    ) {
        let mut ids: Vec<String> = self.computers.keys().cloned().collect();
        if !ids.iter().any(|id| id == computer) {
            ids.push(computer.into());
        }
        let set = |d: &mut Data, id: &str, on: bool| {
            toggle(skip(d.computers.entry(id.into()).or_default()), item, !on)
        };
        let wants = |d: &mut Data, id: &str| {
            shared(d).iter().any(|x| x == item)
                && !skip(d.computers.entry(id.into()).or_default())
                    .iter()
                    .any(|x| x == item)
        };
        if on && !ids.iter().any(|id| wants(self, id)) {
            toggle(shared(self), item, true);
            for id in &ids {
                set(self, id, id == computer || owners.contains(id));
            }
        } else {
            set(self, computer, on);
        }
        if !ids.iter().any(|id| wants(self, id)) {
            toggle(shared(self), item, false);
            for id in &ids {
                set(self, id, true);
            }
        }
    }

    /// `installed_here`: the app is on `host`, which then counts as already having it.
    pub fn place_app(
        &mut self,
        host: &str,
        installed_here: bool,
        computer: &str,
        app: &str,
        on: bool,
    ) {
        let owners = if installed_here {
            vec![host.to_string()]
        } else {
            Vec::new()
        };
        self.place(
            |d| &mut d.apps,
            |c| &mut c.skip_apps,
            &owners,
            computer,
            app,
            on,
        );
    }

    pub fn place_plugin(&mut self, computer: &str, plugin: &str, on: bool) {
        let owners: Vec<String> = self
            .computers
            .iter()
            .filter(|(_, c)| c.plugins.iter().any(|p| p == plugin))
            .map(|(id, _)| id.clone())
            .collect();
        self.place(
            |d| &mut d.plugins,
            |c| &mut c.skip_plugins,
            &owners,
            computer,
            plugin,
            on,
        );
    }

    /// Whether the current theme syncs here: its switch is on and not kept separate here.
    pub fn uses_theme(&self, host: &str) -> bool {
        const THEMES: &str = ".config/omarchy/themes";
        self.folders.iter().any(|f| f == THEMES) && self.uses_folder(host, THEMES)
    }

    /// Synced plugins this computer wants and doesn't have yet, while plugins sync at all.
    pub fn missing_plugins(&self, host: &str) -> Vec<String> {
        if !self.folders.iter().any(|f| f == PLUGIN_DIR) {
            return Vec::new();
        }
        let mine = self
            .computers
            .get(host)
            .map(|c| c.plugins.as_slice())
            .unwrap_or_default();
        let others: BTreeSet<&String> = self
            .computers
            .iter()
            .filter(|(id, _)| *id != host)
            .flat_map(|(_, c)| c.plugins.iter())
            .collect();
        self.plugins
            .iter()
            // IDs come from the shared file, so only plain folder names become paths. Only
            // plugins another computer has can arrive.
            .filter(|p| {
                is_plugin_id(p)
                    && !mine.contains(p)
                    && others.contains(p)
                    && self.wants_plugin(host, p)
            })
            .cloned()
            .collect()
    }

    /// Whether a file (its path under home) syncs on this computer: its most specific switch
    /// (a nested one over its folder's) is on, and this computer doesn't leave it out.
    pub fn syncs(&self, host: &str, target: &str) -> bool {
        // The icons go with the web apps.
        let as_app = target
            .strip_prefix(ICON_DIR)
            .map(|rest| format!("{APPS_DIR}{rest}"));
        let target = as_app.as_deref().unwrap_or(target);
        FOLDERS
            .iter()
            .map(|(f, _, _)| *f)
            .filter(|f| target == *f || target.starts_with(&format!("{f}/")))
            .max_by_key(|f| f.len())
            .is_some_and(|f| self.folders.iter().any(|x| x == f) && self.uses_folder(host, f))
    }

    /// Folders this computer should send and receive.
    pub fn active_folders(&self, host: &str) -> Vec<String> {
        self.folders
            .iter()
            .filter(|f| self.uses_folder(host, f))
            .cloned()
            .collect()
    }
}

/// A plain file name, safe to write into the ignore rules: no folders, wildcards or rule syntax.
fn is_file_name(n: &str) -> bool {
    !n.is_empty()
        && !n.starts_with(['.', '!', '#'])
        && !n.contains(['/', '\\', '*', '?', '[', ']', '{', '}'])
        && !n.chars().any(char::is_control)
}

/// Whether a computer other than `host` already goes by `name` (ignoring case and spaces at
/// either end).
pub fn name_taken(data: &Data, host: &str, name: &str) -> bool {
    let name = name.trim().to_lowercase();
    data.computers
        .iter()
        .any(|(id, c)| id != host && c.name.trim().to_lowercase() == name)
}

pub fn is_plugin_id(id: &str) -> bool {
    !id.is_empty() && !id.starts_with('.') && !id.contains(['/', '\\'])
}

/// A plugin as people see it: its manifest's name and description.
#[derive(Clone, Debug, Serialize)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// Adds or removes `item` so that `list.contains(item) == present`.
pub fn toggle(list: &mut Vec<String>, item: &str, present: bool) {
    list.retain(|x| x != item);
    if present {
        list.push(item.into());
    }
}

#[derive(Clone)]
pub struct Ctx {
    pub home: PathBuf,
    pub src: PathBuf,
    /// This computer's ID: the first 12 characters of /etc/machine-id, which every systemd Linux
    /// has and which differs per installation even when hostnames match. The ignore template
    /// reads the same file.
    pub host: String,
    pub hostname: String,
}

impl Ctx {
    pub fn load() -> Ctx {
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let data_home = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".local/share"));
        let hostname = run(None, "uname", &["-n"]).unwrap_or_default();
        let hostname = hostname.split('.').next().unwrap_or("").to_string();
        // OMARCHY_SYNC_HOST stands in for the machine ID where several "computers" share one
        // machine (the end-to-end tests); the ignore rules read it too. Nothing else sets it.
        let id = std::env::var("OMARCHY_SYNC_HOST")
            .or_else(|_| std::fs::read_to_string("/etc/machine-id"))
            .unwrap_or_default();
        let id: String = id.trim().chars().take(12).collect();
        Ctx {
            // shortcut: assumes chezmoi's default source folder, upgrade to `chezmoi source-path` if anyone sets sourceDir.
            src: data_home.join("chezmoi"),
            host: if id.is_empty() { hostname.clone() } else { id },
            hostname,
            home,
        }
    }

    fn data_path(&self) -> PathBuf {
        self.src.join(".chezmoidata.yaml")
    }

    /// Whether the sync choices differ from the last saved sync. Order doesn't count, so a
    /// switch flipped and flipped back is no change.
    pub fn choices_changed(&self) -> bool {
        let read = |text: &str| {
            serde_yaml::from_str::<Data>(text).ok().map(|mut d| {
                d.upgrade();
                d.sorted()
            })
        };
        let saved = self.git(&["show", "HEAD:.chezmoidata.yaml"]).ok();
        let now = std::fs::read_to_string(self.data_path()).ok();
        saved.as_deref().and_then(read) != now.as_deref().and_then(read)
    }

    /// Plugin folders on this computer.
    /// Web app shortcuts here (they open omarchy-launch-webapp) and the icon files they use.
    pub fn local_web_apps(&self) -> (Vec<String>, Vec<String>) {
        let names = |dir: &str| -> Vec<String> {
            std::fs::read_dir(self.home.join(dir))
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| is_file_name(n))
                .collect()
        };
        let icon_files = names(ICON_DIR);
        let (mut apps, mut icons) = (Vec::new(), Vec::new());
        for name in names(APPS_DIR)
            .into_iter()
            .filter(|n| n.ends_with(".desktop"))
        {
            let text =
                std::fs::read_to_string(self.home.join(APPS_DIR).join(&name)).unwrap_or_default();
            let web = text.lines().any(|l| {
                l.starts_with("Exec=")
                    && (l.contains("omarchy-launch-webapp") || l.contains("--app="))
            });
            if !web {
                continue;
            }
            if let Some(icon) = text.lines().find_map(|l| l.strip_prefix("Icon=")) {
                icons.extend(
                    icon_files
                        .iter()
                        .filter(|f| {
                            f.rsplit_once('.')
                                .is_some_and(|(stem, _)| stem == icon.trim())
                        })
                        .cloned(),
                );
            }
            apps.push(name);
        }
        apps.sort();
        icons.sort();
        icons.dedup();
        (apps, icons)
    }

    pub fn local_plugins(&self) -> Vec<String> {
        let mut ids: Vec<String> = std::fs::read_dir(self.home.join(PLUGIN_DIR))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|id| is_plugin_id(id))
            .collect();
        ids.sort();
        ids
    }

    /// Names a plugin from its manifest, here or (for one this computer doesn't have) in the
    /// sync folder.
    pub fn plugin(&self, id: &str) -> Plugin {
        let manifest = [
            self.home.join(PLUGIN_DIR).join(id),
            self.src.join("dot_config/omarchy/plugins").join(id),
        ]
        .iter()
        .find_map(|dir| std::fs::read(dir.join("manifest.json")).ok())
        .and_then(|m| serde_json::from_slice::<serde_json::Value>(&m).ok())
        .unwrap_or_default();
        let text = |k: &str| manifest[k].as_str().unwrap_or_default().trim().to_string();
        let name = text("name");
        Plugin {
            id: id.to_string(),
            name: if name.is_empty() {
                id.to_string()
            } else {
                name
            },
            description: text("description"),
        }
    }

    pub fn load_data(&self) -> Data {
        let mut data: Data = std::fs::read_to_string(self.data_path())
            .ok()
            .and_then(|s| serde_yaml::from_str(&s).ok())
            .unwrap_or_default();
        data.adopt_legacy(&self.host, &self.hostname);
        if data.version < 2 {
            // So this computer's plugins count among those that synced before.
            data.computers.entry(self.host.clone()).or_default().plugins = self.local_plugins();
        }
        data.upgrade();
        data
    }

    /// Written whole and then renamed into place, so a sync that reads it at the same moment
    /// (the background timer, say) never sees half a file.
    pub fn save_data(&self, data: &Data) -> Result<(), String> {
        let yaml = serde_yaml::to_string(data).map_err(|e| e.to_string())?;
        // A dot name: chezmoi doesn't look at those, and it's gone before git looks.
        let tmp = self.src.join(".omarchy-sync-data.tmp");
        std::fs::write(&tmp, yaml).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, self.data_path()).map_err(|e| e.to_string())
    }

    /// Fails when the choices file is there but can't be read (a typo in a hand edit, a field
    /// a newer version changed): saving over it would turn every choice off, on every computer.
    pub fn data_readable(&self) -> Result<(), String> {
        match std::fs::read_to_string(self.data_path()) {
            Ok(text) if serde_yaml::from_str::<Data>(&text).is_err() => {
                Err("the sync choices file can't be read".into())
            }
            _ => Ok(()),
        }
    }

    fn git(&self, args: &[&str]) -> Result<String, String> {
        run(Some(&self.src), "git", args)
    }

    fn merging(&self) -> bool {
        self.src.join(".git/MERGE_HEAD").exists()
    }

    fn rebasing(&self) -> bool {
        let git_dir = self.src.join(".git");
        git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
    }

    fn conflicted(&self) -> Vec<String> {
        self.git(&["diff", "--name-only", "--diff-filter=U"])
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }

    fn theme_file(&self) -> PathBuf {
        let state = std::env::var("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or(self.home.join(".local/state"));
        state.join("omarchy/current/theme.name")
    }

    /// The image the desktop shows now (Omarchy links it from its state folder).
    pub fn current_background(&self) -> Option<PathBuf> {
        let link = self.theme_file().with_file_name("background");
        std::fs::canonicalize(link).ok().filter(|p| p.is_file())
    }

    pub fn current_theme(&self) -> Option<String> {
        std::fs::read_to_string(self.theme_file())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
}

pub fn run(dir: Option<&Path>, prog: &str, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(prog);
    // English messages to read, in UTF-8: under plain "C", Qt-based tools (omarchy plugin)
    // complain and fail, and file names with non-English letters come out escaped.
    cmd.args(args).stdin(Stdio::null()).env("LC_ALL", "C.UTF-8");
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let out = cmd.output().map_err(|e| format!("{prog}: {e}"))?;
    if out.status.success() {
        // Only the end: git status starts lines with a meaningful space.
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("{prog} {}: {}", args.join(" "), err.trim()))
    }
}

// Besides plain step names, a job's `step` callback carries two kinds of line, read by the
// page (state.ts): the steps it's about to take, and how far along the current one is.
const PLAN: &str = "\u{1}plan\t";
const PERCENT: &str = "\u{1}percent\t";

/// Announces every step a job will take, so the page can show what's still to come.
pub(crate) fn plan(step: &dyn Fn(&str), steps: &[&str]) {
    step(&format!("{PLAN}{}", steps.join("\t")));
}

/// The percentage in a git progress line, for the transfer itself only (git also counts and
/// compresses objects, each from 0 to 100%).
fn transfer_percent(line: &str) -> Option<u8> {
    if !line.contains("Receiving objects") && !line.contains("Writing objects") {
        return None;
    }
    let before = &line[..line.find('%')?];
    let digits = before.trim_end().rsplit(' ').next()?;
    digits.parse().ok()
}

/// A git command that talks to GitHub, passing its upload or download percentage on as it goes.
fn git_progress(ctx: &Ctx, args: &[&str], step: &dyn Fn(&str)) -> Result<String, String> {
    use std::io::Read;
    let mut child = Command::new("git")
        .args(args)
        .current_dir(&ctx.src)
        .env("LC_ALL", "C.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git: {e}"))?;
    let mut stderr = child.stderr.take().ok_or("git: no output")?;
    let (mut all, mut line, mut last) = (Vec::new(), Vec::new(), None);
    let mut buf = [0u8; 1024];
    // Progress lines end in \r as they update; each finished one in \n.
    while let Ok(n) = stderr.read(&mut buf)
        && n > 0
    {
        for &b in &buf[..n] {
            all.push(b);
            if b == b'\r' || b == b'\n' {
                let p = transfer_percent(&String::from_utf8_lossy(&line));
                if p.is_some() && p != last {
                    step(&format!("{PERCENT}{}", p.unwrap_or_default()));
                    last = p;
                }
                line.clear();
            } else {
                line.push(b);
            }
        }
    }
    let out = child.wait_with_output().map_err(|e| format!("git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        let err = String::from_utf8_lossy(&all);
        Err(format!("git {}: {}", args.join(" "), err.trim()))
    }
}

/// Runs the tool rather than looking it up, so a broken shim (e.g. mise with no version set) counts as missing.
pub fn has(prog: &str) -> bool {
    run(None, prog, &["--version"]).is_ok()
}

/// Turns a raw command error into a sentence a person can act on.
pub fn friendly(err: &str) -> String {
    let e = err.to_lowercase();
    if e.contains("could not resolve")
        || e.contains("unable to access")
        || e.contains("network is unreachable")
    {
        "Couldn't reach GitHub. Check your internet connection, then try again.".into()
    } else if e.contains("pkexec") && (e.contains("dismissed") || e.contains("not authorized")) {
        "The password window was closed, so nothing was installed.".into()
    } else if e.contains("choices file can't be read") {
        "The file that holds your sync choices can't be read, so nothing was synced. Undoing the last change to it in History fixes it.".into()
    } else if e.contains("can't undo") {
        "This change can't be undone on its own, because later changes build on it.".into()
    } else if e.contains("rejected") || e.contains("non-fast-forward") {
        "Another computer synced at the same moment. Press Sync now again.".into()
    } else if e.contains("isn't the latest release") {
        "A newer release appeared in the meantime. Check for updates again.".into()
    } else if e.contains("gh auth login") {
        "Not signed in to GitHub. Run gh auth login in a terminal, then try again.".into()
    } else if e.contains("checksum") || e.contains("attestation") {
        "The downloaded update couldn't be verified, so nothing was installed.".into()
    } else {
        "Something went wrong. Nothing on this computer was lost.".into()
    }
}

// ---------- First-time setup ----------

#[derive(Clone, Debug, Serialize)]
pub struct SetupState {
    /// Arch packages that must be installed before anything else.
    pub missing: Vec<&'static str>,
    pub signed_in: bool,
    /// Some(login) once signed in; `found` says whether that account already has synced settings.
    pub login: Option<String>,
    pub found: bool,
    pub ready: bool,
}

pub fn probe(ctx: &Ctx) -> SetupState {
    let missing: Vec<&'static str> = [("chezmoi", "chezmoi"), ("gh", "github-cli"), ("git", "git")]
        .into_iter()
        .filter(|(bin, _)| !has(bin))
        .map(|(_, pkg)| pkg)
        .collect();
    let mut s = SetupState {
        ready: false,
        signed_in: false,
        login: None,
        found: false,
        missing,
    };
    if !s.missing.is_empty() {
        return s;
    }
    if is_set_up(ctx) {
        s.ready = true;
        return s;
    }
    s.signed_in = run(None, "gh", &["auth", "status"]).is_ok();
    if s.signed_in {
        s.login = run(None, "gh", &["api", "user", "--jq", ".login"]).ok();
        if let Some(login) = &s.login {
            s.found = run(
                None,
                "gh",
                &["repo", "view", &format!("{login}/{REPO}"), "--json", "name"],
            )
            .is_ok();
        }
    }
    s
}

/// Set up means the GitHub copy exists. A local folder alone is a setup that was interrupted,
/// and pressing Start syncing again finishes it.
pub fn is_set_up(ctx: &Ctx) -> bool {
    ctx.src.join(".git").exists() && ctx.git(&["remote", "get-url", "origin"]).is_ok()
}

pub fn install_helpers(pkgs: &[&str]) -> Result<(), String> {
    let mut args = vec!["pacman", "-S", "--needed", "--noconfirm"];
    args.extend_from_slice(pkgs);
    run(None, "pkexec", &args).map(|_| ())
}

pub fn sign_in() -> Result<(), String> {
    Command::new("xdg-terminal-exec")
        .args([
            "gh",
            "auth",
            "login",
            "--web",
            "--git-protocol",
            "https",
            "--hostname",
            "github.com",
        ])
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// First computer: save its settings into a new private GitHub repo.
pub fn start(ctx: &Ctx, step: &dyn Fn(&str)) -> Result<(), String> {
    plan(
        step,
        &[
            "Getting ready",
            "Saving this computer's settings",
            "Creating your private sync space on GitHub",
        ],
    );
    step("Getting ready");
    run(None, "gh", &["auth", "setup-git"])?;
    step("Saving this computer's settings");
    seed(ctx)?;
    step("Creating your private sync space on GitHub");
    let src = ctx.src.to_string_lossy();
    run(
        None,
        "gh",
        &[
            "repo",
            "create",
            REPO,
            "--private",
            "--source",
            &src,
            "--remote",
            "origin",
            "--push",
        ],
    )?;
    Ok(())
}

/// Creates the local sync folder from this computer's settings and commits it.
fn seed(ctx: &Ctx) -> Result<(), String> {
    run(None, "chezmoi", &["init"])?;
    std::fs::write(ctx.src.join(".chezmoiignore"), IGNORE).map_err(|e| e.to_string())?;
    let mut data = Data {
        folders: FOLDERS
            .iter()
            .filter(|(p, _, on)| *on && ctx.home.join(p).exists())
            .map(|(p, _, _)| p.to_string())
            .collect(),
        theme: ctx.current_theme(),
        ..Default::default()
    };
    data.computers.entry(ctx.host.clone()).or_default().name = ctx.hostname.clone();
    ctx.save_data(&data)?;
    stage(ctx, &data, &|_| {})?;
    commit(ctx)
}

/// Another computer already syncs: back up this one, then take the synced settings.
pub fn join(ctx: &Ctx, login: &str, step: &dyn Fn(&str)) -> Result<PathBuf, String> {
    plan(
        step,
        &[
            "Getting ready",
            "Downloading your synced settings",
            "Backing up this computer's current settings",
            "Updating this computer",
        ],
    );
    step("Getting ready");
    run(None, "gh", &["auth", "setup-git"])?;
    adopt(ctx, &format!("https://github.com/{login}/{REPO}.git"), step)
}

fn adopt(ctx: &Ctx, url: &str, step: &dyn Fn(&str)) -> Result<PathBuf, String> {
    step("Downloading your synced settings");
    run(None, "chezmoi", &["init", url])?;
    let mut data = ctx.load_data();
    // Joining with an older Omarchy than the others: its own settings stay as they are.
    note_omarchy(&mut data, &ctx.host);
    let paused = data.should_pause(&ctx.host);
    data.computers.entry(ctx.host.clone()).or_default().paused = paused;
    ctx.save_data(&data)?;
    step("Backing up this computer's current settings");
    let backup = backup(ctx, &data.folders, &format!("before-first-sync-{}", now()))?;
    step("Updating this computer");
    run(None, "chezmoi", &["apply", "--force"])?;
    Ok(backup)
}

fn backup(ctx: &Ctx, folders: &[String], name: &str) -> Result<PathBuf, String> {
    let dir = ctx.home.join(".local/share/omarchy-sync/backups");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join(format!("{name}.tar.gz"));
    let existing: Vec<&str> = folders
        .iter()
        .map(|f| f.as_str())
        .filter(|f| ctx.home.join(f).exists())
        .collect();
    if !existing.is_empty() {
        let (f, h) = (file.to_string_lossy(), ctx.home.to_string_lossy());
        let mut args = vec!["-czf", &f, "-C", &h];
        args.extend(existing);
        run(None, "tar", &args)?;
    }
    Ok(file)
}

// ---------- Syncing ----------

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

/// A chezmoi source name without the attribute prefixes it marks files with.
fn strip_attributes(name: &str) -> String {
    let mut name = name.to_string();
    while let Some(rest) = [
        "private_",
        "readonly_",
        "empty_",
        "executable_",
        "literal_",
        "exact_",
    ]
    .iter()
    .find_map(|p| name.strip_prefix(p))
    {
        name = rest.to_string();
    }
    name
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

fn stage(ctx: &Ctx, data: &Data, step: &dyn Fn(&str)) -> Result<(), String> {
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

fn commit(ctx: &Ctx) -> Result<(), String> {
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

#[derive(Clone, Copy, PartialEq, Debug, Deserialize)]
pub enum Prefer {
    Ask,
    ThisComputer,
    OtherComputer,
}

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

/// Only one sync at a time, whether it comes from the window or the background timer.
pub(crate) fn lock(ctx: &Ctx) -> Result<std::fs::File, String> {
    let file =
        std::fs::File::create(ctx.src.join(".git/omarchy-sync.lock")).map_err(|e| e.to_string())?;
    file.try_lock()
        .map_err(|_| "another sync is running".to_string())?;
    Ok(file)
}

/// Waits for any running sync (window or background timer) to finish, then holds the lock.
pub(crate) fn lock_wait(ctx: &Ctx) -> Result<std::fs::File, String> {
    let file =
        std::fs::File::create(ctx.src.join(".git/omarchy-sync.lock")).map_err(|e| e.to_string())?;
    file.lock().map_err(|e| e.to_string())?;
    Ok(file)
}

/// Loads the shared choices, applies one change and saves them, once any running sync (the
/// background timer's, say) is done: a switch flipped during a sync would otherwise be saved
/// over by it, then committed and pushed as the old value.
pub fn edit(ctx: &Ctx, change: impl FnOnce(&mut Data, &str)) -> Result<Data, String> {
    let _lock = lock_wait(ctx)?;
    ctx.data_readable()?;
    let mut data = ctx.load_data();
    change(&mut data, &ctx.host);
    ctx.save_data(&data)?;
    Ok(data)
}

/// Commits this computer's current files (and theme) so nothing local is ever overwritten unsaved.
/// Returns this computer's new name when it had to differ from the hostname, which another
/// computer already uses.
fn save_local(ctx: &Ctx, step: &dyn Fn(&str)) -> Result<Option<String>, String> {
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
fn versions(ctx: &Ctx, source: &str) -> Versions {
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
fn clash(ctx: &Ctx, files: Vec<String>, broken: Vec<String>) -> SyncError {
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
    let mut report = sync(ctx, Prefer::Ask, install, step)?;
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

/// Records this computer's Omarchy version.
fn note_omarchy(data: &mut Data, host: &str) {
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
fn save_newly_synced(ctx: &Ctx, synced_before: &[String]) -> Result<Option<String>, String> {
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
pub fn sync(
    ctx: &Ctx,
    prefer: Prefer,
    install: bool,
    step: &dyn Fn(&str),
) -> Result<Report, SyncError> {
    write_status(ctx, "syncing", "Syncing…");
    let result = sync_inner(ctx, prefer, install, step);
    match &result {
        Ok(_) => {
            let data = ctx.load_data();
            let installed = installed();
            let waiting = missing_apps(&data, &ctx.host, &installed).len()
                + missing_scripted(&data, &ctx.host, &installed).len()
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
    result
}

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
        include_str!("../../packaging/bar-plugin/manifest.json"),
    ),
    (
        "Indicator2.qml",
        include_str!("../../packaging/bar-plugin/Indicator2.qml"),
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
pub fn ensure_bar_indicator(ctx: &Ctx) -> Result<(), String> {
    let off = ctx
        .load_data()
        .computers
        .get(&ctx.host)
        .is_some_and(|c| c.no_bar_indicator);
    let dir = ctx.home.join(PLUGIN_DIR).join(BAR_PLUGIN);
    let current = BAR_FILES
        .iter()
        .all(|(name, text)| std::fs::read_to_string(dir.join(name)).is_ok_and(|t| t == *text));
    // Only whether the shell's command is there: it answers `--version` with its usage and
    // exit code 1, so `has` would say it's missing on every Omarchy.
    let shell = Command::new("omarchy-shell")
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .is_ok();
    if off || current || !shell {
        return Ok(());
    }
    set_bar_indicator(ctx, true)
}

/// Puts the indicator on the bar, or takes it off, through Omarchy's own plugin commands.
pub fn set_bar_indicator(ctx: &Ctx, on: bool) -> Result<(), String> {
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
        run(None, "omarchy-shell", &["shell", "rescanPlugins"])?;
        // The shell scans in the background, so the new plugin may take a moment to be known.
        let mut enabled = Err(String::new());
        for _ in 0..20 {
            enabled = run(None, "omarchy", &["plugin", "enable", BAR_PLUGIN]);
            if enabled.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        enabled?;
        // The shell caches each plugin folder's files from when it first read them, so a new
        // or changed widget file only loads after a restart (about a second; the bar redraws).
        run(None, "omarchy", &["restart", "shell"]).map(|_| ())
    } else {
        // Already off (or never on) is fine: the goal is that it's gone.
        let _ = run(None, "omarchy", &["plugin", "disable", BAR_PLUGIN]);
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        let _ = run(None, "omarchy-shell", &["shell", "rescanPlugins"]);
        Ok(())
    }
}

fn sync_inner(
    ctx: &Ctx,
    prefer: Prefer,
    install: bool,
    step: &dyn Fn(&str),
) -> Result<Report, SyncError> {
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
        let mut args = vec!["rebase", "-q"];
        // During a rebase, "theirs" is this computer's commits and "ours" is what came from GitHub.
        match prefer {
            Prefer::ThisComputer => args.extend(["-X", "theirs"]),
            Prefer::OtherComputer => args.extend(["-X", "ours"]),
            Prefer::Ask => {}
        }
        args.push("@{u}");
        let before = ctx.git(&["rev-parse", "HEAD"]).map_err(fail)?;
        let replayed = if caught_up {
            Ok(String::new())
        } else {
            ctx.git(&args)
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

    let missing = missing_apps(&data, &ctx.host, &installed());
    if missing.is_empty() || !install {
        return Ok(Report {
            notes,
            waiting: missing,
        });
    }
    step(&format!("Installing {}", plural(missing.len(), "app")));
    match install_apps(&missing) {
        Ok(()) => notes.push(format!("Installed {}.", missing.join(", "))),
        Err(e) => notes.push(format!(
            "Couldn't install {}. {}",
            missing.join(", "),
            friendly(&e)
        )),
    }
    Ok(Report {
        notes,
        waiting: vec![],
    })
}

/// Synced apps this computer should have but doesn't.
/// Synced apps this computer wants and `yay` can install (not ones from outside the repos).
pub fn missing_apps(data: &Data, host: &str, installed: &BTreeSet<String>) -> Vec<String> {
    let here = repos_here();
    data.apps
        .iter()
        .filter(|a| {
            data.wants_app(host, a)
                && !installed.contains(*a)
                && !data.outside.contains_key(*a)
                // From an extra repository this computer doesn't have: never from the AUR
                // instead; it waits for the repository (see missing_repos).
                && data.app_repo.get(*a).is_none_or(|r| here.contains(r))
        })
        .cloned()
        .collect()
}

/// The package repositories this computer uses.
pub fn repos_here() -> BTreeSet<String> {
    run(None, "pacman-conf", &["--repo-list"])
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// An extra repository this computer needs for apps it should have.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MissingRepo {
    pub name: String,
    pub repository: Repository,
    pub apps: Vec<String>,
}

/// Extra repositories this computer lacks for synced apps it wants and doesn't have yet.
pub fn missing_repos(
    data: &Data,
    host: &str,
    installed: &BTreeSet<String>,
    here: &BTreeSet<String>,
) -> Vec<MissingRepo> {
    let mut needed: BTreeMap<&String, Vec<String>> = BTreeMap::new();
    for app in data
        .apps
        .iter()
        .filter(|a| data.wants_app(host, a) && !installed.contains(*a))
    {
        if let Some(repo) = data.app_repo.get(app).filter(|r| !here.contains(*r)) {
            needed.entry(repo).or_default().push(app.clone());
        }
    }
    needed
        .into_iter()
        .filter_map(|(name, apps)| {
            Some(MissingRepo {
                name: name.clone(),
                repository: data.repositories.get(name)?.clone(),
                apps,
            })
        })
        .collect()
}

/// The extra repositories this computer has that are shared, with the person's apps from each.
pub fn repos_with_apps(data: &Data, here: &BTreeSet<String>) -> Vec<MissingRepo> {
    data.repositories
        .iter()
        .filter(|(name, _)| here.contains(*name))
        .map(|(name, repository)| MissingRepo {
            name: name.clone(),
            repository: repository.clone(),
            apps: data
                .app_repo
                .iter()
                .filter(|(_, r)| *r == name)
                .map(|(app, _)| app.clone())
                .collect(),
        })
        .collect()
}

/// This computer's extra repositories as pacman uses them (servers and signature rules), with
/// the key that signs each, and which of the person's apps come from one.
fn record_repositories(data: &mut Data) {
    let extra: Vec<String> = repos_here()
        .into_iter()
        .filter(|r| !STANDARD_REPOS.contains(&r.as_str()))
        .collect();
    if extra.is_empty() {
        return;
    }
    for name in &extra {
        let conf = run(None, "pacman-conf", &[&format!("--repo={name}")]).unwrap_or_default();
        let field = |key: &str| -> Vec<String> {
            conf.lines()
                .filter_map(|l| l.strip_prefix(key))
                .map(|v| v.trim().to_string())
                .collect()
        };
        let repository = Repository {
            servers: field("Server = "),
            sig_level: field("SigLevel = "),
            key: repo_key(name),
        };
        if !repository.servers.is_empty() {
            data.repositories.insert(name.clone(), repository);
        }
    }
    let mine = user_apps_here();
    for (app, repo) in sources(mine.iter(), &[], &BTreeMap::new()) {
        if extra.contains(&repo) {
            data.app_repo.insert(app, repo);
        }
    }
}

/// The key that signed a repository's package list, copied from pacman's keyring. The keyring
/// is root's, so its public half is read from a copy in a temporary folder.
fn repo_key(name: &str) -> Option<RepoKey> {
    let db = run(None, "pacman-conf", &["DBPath"]).unwrap_or("/var/lib/pacman/".into());
    let sig = Path::new(db.trim())
        .join("sync")
        .join(format!("{name}.db.sig"));
    let packets = run(None, "gpg", &["--list-packets", &sig.to_string_lossy()]).ok()?;
    let fingerprint = packets
        .split_whitespace()
        .skip_while(|w| *w != "fpr")
        .nth(2)?
        .trim_end_matches(')')
        .to_uppercase();
    if fingerprint.len() != 40 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let dir = std::env::temp_dir().join(format!("omarchy-sync-keys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let ring = ["pubring.gpg", "pubring.kbx"]
        .iter()
        .find(|f| std::fs::copy(Path::new("/etc/pacman.d/gnupg").join(f), dir.join(f)).is_ok());
    let home = dir.to_string_lossy().into_owned();
    let gpg = |args: &[&str]| {
        let mut all = vec!["--homedir", home.as_str(), "--no-autostart", "--batch"];
        all.extend_from_slice(args);
        run(None, "gpg", &all).ok()
    };
    let key = ring.and_then(|_| {
        let armored = gpg(&["--export", "--armor", &fingerprint])?;
        let owner = gpg(&["--list-keys", "--with-colons", &fingerprint])?
            .lines()
            .find_map(|l| {
                l.strip_prefix("uid:")
                    .map(|u| u.split(':').nth(8).unwrap_or("").to_string())
            })
            .unwrap_or_default();
        armored
            .starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----")
            .then(|| RepoKey {
                fingerprint: fingerprint.clone(),
                owner,
                armored,
            })
    });
    let _ = std::fs::remove_dir_all(&dir);
    key
}

/// Adds an extra repository on this computer, as recorded from another one: its settings file
/// in /etc/pacman.d, an Include for it in pacman.conf, its key imported and trusted, and the
/// package lists refreshed. One password prompt (pkexec); every value is checked first.
pub fn add_repository(data: &Data, name: &str) -> Result<(), String> {
    let repo = data
        .repositories
        .get(name)
        .ok_or_else(|| format!("no repository called {name}"))?;
    let plain = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    if !plain(name) || STANDARD_REPOS.contains(&name) {
        return Err(format!("not a repository that can be added: {name}"));
    }
    if repos_here().contains(name) {
        return Ok(());
    }
    const LEVELS: &[&str] = &[
        "Never",
        "Optional",
        "Required",
        "TrustedOnly",
        "TrustAll",
        "PackageNever",
        "PackageOptional",
        "PackageRequired",
        "PackageTrustedOnly",
        "PackageTrustAll",
        "DatabaseNever",
        "DatabaseOptional",
        "DatabaseRequired",
        "DatabaseTrustedOnly",
        "DatabaseTrustAll",
    ];
    let servers_ok = !repo.servers.is_empty()
        && repo.servers.iter().all(|s| {
            s.starts_with("https://") && !s.chars().any(|c| c.is_whitespace() || c.is_control())
        });
    if !servers_ok || !repo.sig_level.iter().all(|l| LEVELS.contains(&l.as_str())) {
        return Err(format!("the settings recorded for {name} don't look right"));
    }
    let dir = std::env::temp_dir().join(format!("omarchy-sync-repo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut conf = format!("[{name}]\n");
    if !repo.sig_level.is_empty() {
        conf.push_str(&format!("SigLevel = {}\n", repo.sig_level.join(" ")));
    }
    for server in &repo.servers {
        conf.push_str(&format!("Server = {server}\n"));
    }
    let target = format!("/etc/pacman.d/omarchy-sync-{name}.conf");
    let include = format!("Include = {target}");
    let mut script = format!(
        "set -e\ninstall -m 644 '{}' '{target}'\ngrep -qxF '{include}' /etc/pacman.conf || printf '\\n%s\\n' '{include}' >> /etc/pacman.conf\n",
        dir.join("repo.conf").display()
    );
    std::fs::write(dir.join("repo.conf"), conf).map_err(|e| e.to_string())?;
    if let Some(key) = &repo.key {
        let fpr_ok =
            key.fingerprint.len() == 40 && key.fingerprint.bytes().all(|b| b.is_ascii_hexdigit());
        if !fpr_ok
            || !key
                .armored
                .starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----")
        {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(format!("the key recorded for {name} doesn't look right"));
        }
        std::fs::write(dir.join("repo.key"), &key.armored).map_err(|e| e.to_string())?;
        script.push_str(&format!(
            "pacman-key --add '{}'\npacman-key --lsign-key {}\n",
            dir.join("repo.key").display(),
            key.fingerprint
        ));
    }
    script.push_str("pacman -Sy --noconfirm\n");
    std::fs::write(dir.join("add.sh"), &script).map_err(|e| e.to_string())?;
    let result = run(
        None,
        "pkexec",
        &["bash", &dir.join("add.sh").to_string_lossy()],
    )
    .map(|_| ());
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Synced apps from outside the repos that this computer wants, with their install scripts.
pub fn missing_scripted(
    data: &Data,
    host: &str,
    installed: &BTreeSet<String>,
) -> Vec<(String, Script)> {
    data.apps
        .iter()
        .filter(|a| data.wants_app(host, a) && !installed.contains(*a))
        .filter_map(|a| Some((a.clone(), data.outside.get(a)?.clone()?)))
        .collect()
}

/// Which of these locally installed packages came from neither Arch's repos nor the AUR.
/// Asks the AUR once per name and remembers it; when it can't be reached, it claims none.
pub fn outside_repos(mine: &[String]) -> Vec<String> {
    use std::sync::{Mutex, OnceLock};
    static IN_AUR: OnceLock<Mutex<BTreeMap<String, bool>>> = OnceLock::new();
    let foreign: BTreeSet<String> = run(None, "pacman", &["-Qqm"])
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    let candidates: Vec<&String> = mine.iter().filter(|a| foreign.contains(*a)).collect();
    let known = IN_AUR.get_or_init(Default::default);
    let unknown: Vec<&String> = {
        let k = known.lock().unwrap_or_else(|e| e.into_inner());
        candidates
            .iter()
            .copied()
            .filter(|a| !k.contains_key(*a))
            .collect()
    };
    if !unknown.is_empty() {
        let query: Vec<String> = unknown
            .iter()
            .map(|a| format!("arg[]={}", a.replace('+', "%2B").replace('@', "%40")))
            .collect();
        let url = format!("https://aur.archlinux.org/rpc/v5/info?{}", query.join("&"));
        let Ok(body) = run(None, "curl", &["-fsS", "--max-time", "8", &url]) else {
            return Vec::new();
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&body) else {
            return Vec::new();
        };
        let found: BTreeSet<&str> = json["results"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["Name"].as_str())
            .collect();
        let mut k = known.lock().unwrap_or_else(|e| e.into_inner());
        for a in unknown {
            k.insert(a.clone(), found.contains(a.as_str()));
        }
    }
    let k = known.lock().unwrap_or_else(|e| e.into_inner());
    candidates
        .into_iter()
        .filter(|a| k.get(*a) == Some(&false))
        .cloned()
        .collect()
}

/// Where apps come from. pacman takes a package from the first repository in pacman.conf that
/// has it (on Omarchy: omarchy, then Arch's core, extra and multilib), and yay turns to the AUR
/// only when none does; apps from none of these install with their steps.
pub fn sources<'a>(
    apps: impl Iterator<Item = &'a String>,
    outside_here: &[String],
    outside_synced: &BTreeMap<String, Option<Script>>,
) -> BTreeMap<String, String> {
    sources_with(apps, outside_here, outside_synced, &BTreeMap::new())
}

/// `sources`, where an app that comes from an extra repository this computer doesn't have
/// (`app_repo`, recorded by another computer) shows that repository rather than the AUR.
pub fn sources_with<'a>(
    apps: impl Iterator<Item = &'a String>,
    outside_here: &[String],
    outside_synced: &BTreeMap<String, Option<Script>>,
    app_repo: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut repo_of: BTreeMap<&str, &str> = BTreeMap::new();
    let listing = run(None, "pacman", &["-Sl"]).unwrap_or_default();
    // `pacman -Sl` lists repositories in pacman.conf's order; the first one listing a package wins.
    for line in listing.lines() {
        let mut parts = line.split(' ');
        if let (Some(repo), Some(name)) = (parts.next(), parts.next()) {
            repo_of.entry(name).or_insert(repo);
        }
    }
    apps.map(|app| {
        let source = match repo_of.get(app.as_str()) {
            Some(&"omarchy") => "omarchy",
            Some(&("core" | "extra" | "multilib")) => "arch",
            Some(repo) => repo,
            None if outside_here.contains(app) || outside_synced.contains_key(app) => "elsewhere",
            None if app_repo.contains_key(app) => app_repo[app].as_str(),
            None => "aur",
        };
        (app.clone(), source.to_string())
    })
    .collect()
}

/// A pacman package name: lowercase letters, digits and @._+-, not starting with - or a dot.
pub fn is_package_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "@._+-".contains(c))
}

/// Installs an app with its install command, run in a fresh empty folder in a terminal window,
/// so the person sees what it does and can type a password if it asks. Waits for the command
/// to finish, then reports how it ended.
pub fn install_with_script(
    ctx: &Ctx,
    app: &str,
    script: &Script,
    step: &dyn Fn(&str),
) -> Result<(), String> {
    // The name comes from the shared file, which anyone can edit on GitHub: only a plain
    // package name becomes a folder that is emptied below.
    if !is_package_name(app) {
        return Err(format!("not an app: {app}"));
    }
    let dir = ctx.home.join(".cache/omarchy-sync/install").join(app);
    let done = dir.with_extension("done");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_file(&done);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    step(&format!("Installing {app} in a terminal window"));
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    // The steps run as one script that stops at the first failing step, so a failed build
    // never goes on to install.
    let file = dir.with_extension("sh");
    std::fs::write(
        &file,
        format!(
            "set -e -o pipefail\ncd {}\n{}\n",
            quote(&dir.to_string_lossy()),
            script.command
        ),
    )
    .map_err(|e| e.to_string())?;
    let line = format!(
        "bash {}; echo $? > {}; echo; read -rp 'Press Enter to close this window. ' _",
        quote(&file.to_string_lossy()),
        quote(&done.to_string_lossy()),
    );
    Command::new("xdg-terminal-exec")
        .args(["bash", "-c", &line])
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("xdg-terminal-exec: {e}"))?;
    // Some terminals hand the window to an already-running copy and return at once, so the
    // command's own end is the signal: it writes its exit code to `done`.
    for _ in 0..3600 {
        if let Ok(code) = std::fs::read_to_string(&done) {
            let _ = std::fs::remove_file(&done);
            return match code.trim() {
                "0" => Ok(()),
                code => Err(format!(
                    "the install command for {app} stopped with code {code}"
                )),
            };
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    Err(format!(
        "the install command for {app} didn't finish within an hour"
    ))
}

pub fn install_apps(apps: &[String]) -> Result<(), String> {
    let mut args = vec!["-S", "--needed", "--noconfirm", "--sudo", "pkexec", "--"];
    args.extend(apps.iter().map(|s| s.as_str()));
    run(None, "yay", &args).map(|_| ())
}

// ---------- History and undo ----------

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub sha: String,
    pub computer: String,
    pub when: i64,
    /// Plain names of what changed, e.g. "Keyboard shortcuts".
    pub labels: Vec<String>,
    pub is_undo: bool,
    /// Where two computers' changes were joined after a clash; it can't be undone on its own.
    pub is_merge: bool,
    /// The very first save, which has nothing before it to go back to.
    pub first: bool,
    /// Every file it changed: what Undo would put back.
    pub files: Vec<Change>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Change {
    /// The path under home, or DATA_LABEL for the shared choices.
    pub path: String,
    /// "Added", "Changed" or "Removed".
    pub how: &'static str,
}

/// Reads `git ... --name-status` lines ("M\tpath", "R100\told\tnew") as changes, skipping
/// the ignore rules, which aren't anyone's settings.
fn parse_changes<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<Change> {
    lines
        .filter_map(|l| {
            let mut parts = l.split('\t');
            let status = parts.next()?;
            let file = parts.next_back()?;
            let how = match status.chars().next()? {
                'A' => "Added",
                'D' => "Removed",
                _ => "Changed",
            };
            (!file.starts_with(".chezmoiignore")).then(|| Change {
                path: source_to_target(file),
                how,
            })
        })
        .collect()
}

/// A commit message body naming every change in plain words, grouped like the app shows them.
fn describe(changes: &[Change]) -> String {
    let mut groups: Vec<(&str, Vec<String>)> = Vec::new();
    for c in changes {
        let name = label(&c.path);
        let file = c.path.rsplit('/').next().unwrap_or(&c.path);
        let item = if c.path == DATA_LABEL {
            "updated".to_string()
        } else {
            format!("{} {file}", c.how.to_lowercase())
        };
        match groups.iter_mut().find(|(n, _)| *n == name) {
            Some((_, items)) => items.push(item),
            None => groups.push((name, vec![item])),
        }
    }
    groups
        .iter()
        .map(|(name, items)| format!("{name}: {}", items.join(", ")))
        .collect::<Vec<_>>()
        .join("\n")
}

const UNDO_PREFIX: &str = "Undo: ";

pub fn history(ctx: &Ctx) -> Vec<Entry> {
    let log = ctx
        .git(&[
            "log",
            "-n",
            "50",
            "--name-status",
            "--format=%x1e%H%x09%an%x09%ct%x09%s%x09%P",
        ])
        .unwrap_or_default();
    parse_history(&log)
}

/// Parses `git log --name-status --format=%x1e%H%x09%an%x09%ct%x09%s%x09%P`.
pub fn parse_history(log: &str) -> Vec<Entry> {
    log.split('\x1e')
        .filter_map(|block| {
            let mut lines = block.lines();
            let head: Vec<&str> = lines.next()?.split('\t').collect();
            let [sha, computer, when, subject, parents] = head[..] else {
                return None;
            };
            let files = parse_changes(lines.filter(|l| !l.is_empty()));
            let mut labels: Vec<String> = Vec::new();
            for c in &files {
                let name = label(&c.path).to_string();
                if !labels.contains(&name) {
                    labels.push(name);
                }
            }
            Some(Entry {
                sha: sha.into(),
                computer: computer.into(),
                when: when.parse().ok()?,
                labels,
                is_undo: subject.starts_with(UNDO_PREFIX),
                is_merge: parents.contains(' '),
                first: parents.is_empty(),
                files,
            })
        })
        .collect()
}

/// chezmoi's source name for a file back to its path under home (dot_config -> .config).
pub fn source_to_target(path: &str) -> String {
    if path == ".chezmoidata.yaml" {
        return DATA_LABEL.into();
    }
    if path.starts_with(".background/") {
        return path.into();
    }
    const ATTRS: &[&str] = &[
        "private_",
        "readonly_",
        "empty_",
        "executable_",
        "exact_",
        "symlink_",
        "create_",
        "modify_",
        "encrypted_",
    ];
    path.split('/')
        .map(|part| {
            let mut p = part;
            while let Some(rest) = ATTRS.iter().find_map(|a| p.strip_prefix(a)) {
                p = rest;
            }
            p.strip_prefix("dot_")
                .map_or(p.to_string(), |r| format!(".{r}"))
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Reverses one earlier change. The caller then syncs, which applies it here and shares it.
pub fn undo(ctx: &Ctx, sha: &str) -> Result<(), String> {
    let _lock = lock_wait(ctx)?;
    save_local(ctx, &|_| {})?;
    let synced_before = ctx.load_data().active_folders(&ctx.host);
    if ctx.git(&["revert", "--no-commit", sha]).is_err() {
        let _ = ctx.git(&["revert", "--abort"]);
        return Err("can't undo: later changes depend on it".into());
    }
    // Never puts back a file that's switched off now (the choices as undone decide): it would
    // reach every computer, over copies that were never saved.
    let choices = ctx.load_data();
    let staged = ctx.git(&["diff", "--cached", "--name-status"])?;
    let revives_switched_off = parse_changes(staged.lines()).iter().any(|c| {
        c.how == "Added"
            && c.path != DATA_LABEL
            && !c.path.starts_with(".background/")
            && !choices.syncs(&ctx.host, &c.path)
    });
    if revives_switched_off {
        let _ = ctx.git(&["revert", "--abort"]);
        return Err(
            "This change would put back settings that are switched off, so it can't be undone."
                .into(),
        );
    }
    let who = ctx
        .git(&["log", "-1", "--format=%an", sha])
        .unwrap_or_default();
    ctx.git(&[
        "commit",
        "-q",
        "-m",
        &format!("{UNDO_PREFIX}a change from {who}"),
    ])?;
    save_newly_synced(ctx, &synced_before)?;
    // Put the undone files in place now; otherwise the next sync would re-save the old ones over the undo.
    run(None, "chezmoi", &["apply", "--force"]).map(|_| ())
}

// ---------- Background syncing ----------

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
    let timer = "[Unit]\nDescription=Sync Omarchy settings every 15 minutes\n\n[Timer]\nOnBootSec=3min\nOnUnitActiveSec=15min\n\n[Install]\nWantedBy=timers.target\n";
    let dir = unit_dir(ctx);
    let service_path = dir.join(format!("{UNIT}.service"));
    if enabled && std::fs::read_to_string(&service_path).is_ok_and(|s| s == service) {
        return Ok(());
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(&service_path, service).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(format!("{UNIT}.timer")), timer).map_err(|e| e.to_string())?;
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
fn state_file(ctx: &Ctx) -> PathBuf {
    let state = std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or(ctx.home.join(".local/state"));
    state.join("omarchy-sync/background")
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
    let outcome = match sync(ctx, Prefer::Ask, me.install_without_asking, &|_| {}) {
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

// ---------- What the main screen shows ----------

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
pub fn overview(ctx: &Ctx, step: &dyn Fn(&str), just_synced: bool) -> Result<Overview, String> {
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
    let data = ctx.load_data();
    let online = just_synced || {
        stage(ctx, &data, step)?;
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
    let apps = apps(&data);
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
    let here = repos_here();
    let mut unshared: Vec<String> = parse_status(&status)
        .iter()
        .filter(|p| !p.starts_with(".chezmoiignore"))
        .map(|p| source_to_target(p))
        .collect();
    for change in look_changes(ctx, &data) {
        if !unshared.contains(&change) {
            unshared.push(change);
        }
    }
    Ok(Overview {
        unshared,
        incoming,
        online,
        computers: computers(&parse_log(&log), &data, &ctx.host),
        apps,
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
        missing_repos: missing_repos(&data, &ctx.host, &installed(), &here),
        repos_here: repos_with_apps(&data, &here),
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
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    Ok(())
}

pub fn installed() -> BTreeSet<String> {
    run(None, "pacman", &["-Qq"])
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// The apps installed here by the person: explicitly installed, not part of Omarchy.
fn user_apps_here() -> Vec<String> {
    let mut base: BTreeSet<String> = BTreeSet::new();
    for f in ["omarchy-base.packages", "omarchy-other.packages"] {
        let text =
            std::fs::read_to_string(format!("/usr/share/omarchy/install/{f}")).unwrap_or_default();
        base.extend(
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(String::from),
        );
    }
    let explicit = run(None, "pacman", &["-Qqe"]).unwrap_or_default();
    user_apps(explicit.lines(), &base)
}

fn apps(data: &Data) -> Apps {
    let mine = user_apps_here();
    let installed = installed();
    let mut names: Vec<&str> = mine.iter().map(|s| s.as_str()).collect();
    names.extend(
        data.apps
            .iter()
            .filter(|a| installed.contains(*a) && !mine.contains(a))
            .map(|s| s.as_str()),
    );
    let about = if names.is_empty() {
        BTreeMap::new()
    } else {
        let mut args = vec!["-Qi"];
        args.extend(names);
        parse_info(&run(None, "pacman", &args).unwrap_or_default())
    };
    let outside = outside_repos(&mine);
    // Every app shown: this computer's and the synced ones it may not have yet.
    let all = mine.iter().chain(data.apps.iter());
    let source = sources_with(all, &outside, &data.outside, &data.app_repo);
    Apps {
        installed,
        mine,
        outside,
        source,
        about,
    }
}

/// Explicitly installed packages minus Omarchy's own and base system ones.
pub fn user_apps<'a>(
    explicit: impl Iterator<Item = &'a str>,
    base: &BTreeSet<String>,
) -> Vec<String> {
    explicit
        .filter(|p| !base.contains(*p) && !p.starts_with("omarchy") && !SYSTEM.contains(p))
        .map(String::from)
        .collect()
}

/// Maps paths inside the sync folder to paths under home.
/// The theme and background in use here when they differ from the shared ones. Neither shows
/// in git until a sync records it, so a change made in Omarchy is listed from here.
fn look_changes(ctx: &Ctx, data: &Data) -> Vec<String> {
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
fn computers(last: &BTreeMap<String, i64>, data: &Data, host: &str) -> Vec<(String, Option<i64>)> {
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

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The web address of this computer's sync space, from the repo's GitHub remote.
pub fn sync_space(ctx: &Ctx) -> Option<String> {
    let remote = ctx.git(&["remote", "get-url", "origin"]).ok()?;
    let path = remote
        .strip_prefix("https://github.com/")
        .or_else(|| remote.strip_prefix("git@github.com:"))?
        .trim_end_matches(".git");
    // Only owner/repo made of safe characters: the page opens this in a browser.
    let safe = path.split('/').count() == 2
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c));
    safe.then(|| format!("https://github.com/{path}"))
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
    fn merging_keeps_both_computers_changes() {
        let mut base = Data {
            apps: vec!["a".into(), "b".into()],
            ..Default::default()
        };
        base.computers.entry("x".into()).or_default().name = "X".into();
        base.computers.entry("y".into()).or_default().name = "Y".into();
        let mut theirs = base.clone();
        theirs.apps.push("c".into());
        theirs.computers.get_mut("y").unwrap().plugins = vec!["dock".into()];
        let mut mine = base.clone();
        mine.apps.retain(|a| a != "a");
        mine.computers.get_mut("x").unwrap().plugins = vec!["clock".into()];
        let m = Data::merge(&base, &theirs, &mine);
        assert_eq!(m.apps, vec!["b".to_string(), "c".to_string()]);
        assert_eq!(m.computers["x"].plugins, vec!["clock".to_string()]);
        assert_eq!(m.computers["y"].plugins, vec!["dock".to_string()]);
    }

    #[test]
    fn apps_from_outside_the_repos_install_only_by_script() {
        assert!(is_package_name("numpad") && is_package_name("lib32-gcc+x@1.2"));
        assert!(!is_package_name("-rf") && !is_package_name("a b") && !is_package_name("A"));
        let mut d = Data {
            apps: vec!["numpad".into(), "rustxt".into(), "firefox".into()],
            ..Default::default()
        };
        d.outside.insert("numpad".into(), None);
        let script = Script {
            command: "curl -fsSL https://example.com/install.sh | bash".into(),
            ..Default::default()
        };
        d.outside.insert("rustxt".into(), Some(script.clone()));
        let none = BTreeSet::new();
        // yay only gets what it can install; rustxt waits for its script; numpad can't come.
        assert_eq!(missing_apps(&d, "b", &none), vec!["firefox".to_string()]);
        assert_eq!(
            missing_scripted(&d, "b", &none),
            vec![("rustxt".to_string(), script.clone())]
        );
        // A script added on one computer and an app found on another both survive a merge.
        let base = d.clone();
        let mut mine = d.clone();
        mine.outside.insert("numpad".into(), Some(script.clone()));
        let mut theirs = d.clone();
        theirs.outside.insert("owl".into(), None);
        let m = Data::merge(&base, &theirs, &mine);
        assert_eq!(m.outside.get("numpad"), Some(&Some(script)));
        assert_eq!(m.outside.get("owl"), Some(&None));
    }

    #[test]
    fn an_older_script_becomes_one_command() {
        let mut d = Data {
            version: 3,
            ..Default::default()
        };
        d.outside.insert(
            "numpad".into(),
            Some(Script {
                command: "./install.sh".into(),
                repo: "https://example.com/numpad".into(),
            }),
        );
        d.upgrade();
        assert_eq!(
            d.outside["numpad"].as_ref().map(|s| s.command.as_str()),
            Some("git clone https://example.com/numpad numpad && cd numpad && ./install.sh")
        );
    }

    #[test]
    fn names_are_used_once() {
        let mut d = Data::default();
        d.computers.entry("a".into()).or_default().name = "omarchy".into();
        assert!(name_taken(&d, "b", " Omarchy "));
        assert!(!name_taken(&d, "a", "omarchy"));
        assert!(!name_taken(&d, "b", "laptop"));
        assert_eq!(d.fresh_name("b", "omarchy"), "omarchy 2");
    }

    #[test]
    fn flipping_back_is_no_change() {
        let mut d = Data {
            apps: vec!["b".into(), "a".into()],
            ..Default::default()
        };
        let before = d.clone().sorted();
        toggle(&mut d.apps, "b", false);
        d.set_app("x", "a", false);
        toggle(&mut d.apps, "b", true);
        d.set_app("x", "a", true);
        assert_ne!(d.apps, vec!["b".to_string(), "a".to_string()]);
        assert_eq!(d.sorted(), before);
    }

    #[test]
    fn plugins_from_other_computers_wait_here() {
        let mut d = Data {
            folders: vec![PLUGIN_DIR.into()],
            plugins: vec!["clock".into(), "dock".into(), "../evil".into()],
            ..Default::default()
        };
        d.computers.entry("a".into()).or_default().plugins = vec!["clock".into()];
        d.computers.entry("b".into()).or_default().plugins = vec![
            "clock".into(),
            "dock".into(),
            "solo".into(),
            "../evil".into(),
        ];
        // "solo" isn't synced, so it never waits anywhere.
        assert_eq!(d.missing_plugins("a"), vec!["dock".to_string()]);
        assert_eq!(
            d.missing_plugins("c"),
            vec!["clock".to_string(), "dock".to_string()]
        );
        d.set_plugin("a", "dock", false);
        assert!(d.missing_plugins("a").is_empty());
        d.set_plugin("a", "dock", true);
        d.folders.clear();
        assert!(d.missing_plugins("a").is_empty());
    }

    #[test]
    fn one_switch_per_computer_shares_and_stops_sharing() {
        let mut d = Data::default();
        for id in ["here", "desk", "lap"] {
            d.computers.entry(id.into()).or_default();
        }
        // Only here: switching Desk on shares it with Desk and here, not Lap.
        d.place_app("here", true, "desk", "zed", true);
        assert_eq!(d.apps, vec!["zed".to_string()]);
        assert!(d.wants_app("here", "zed") && d.wants_app("desk", "zed"));
        assert!(!d.wants_app("lap", "zed"));
        d.place_app("here", true, "lap", "zed", true);
        assert!(d.wants_app("lap", "zed"));
        // Every computer switched off: no longer shared, nothing left behind.
        for id in ["lap", "desk", "here"] {
            d.place_app("here", true, id, "zed", false);
        }
        assert!(d.apps.is_empty());
        // With one computer, switching it on shares it for computers added later.
        let mut solo = Data::default();
        solo.computers.entry("here".into()).or_default();
        solo.place_app("here", true, "here", "zed", true);
        assert!(solo.wants_app("here", "zed"));
        assert!(d.computers.values().all(|c| c.skip_apps.is_empty()));
        // A plugin shared from Lap brings Lap along, since its copy is the one shared.
        d.computers.get_mut("lap").unwrap().plugins = vec!["dock".into()];
        d.place_plugin("desk", "dock", true);
        assert!(d.wants_plugin("lap", "dock") && d.wants_plugin("desk", "dock"));
        assert!(!d.wants_plugin("here", "dock"));
    }

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
    fn upgrade_keeps_every_plugin_synced() {
        let mut d = Data::default();
        d.computers.entry("a".into()).or_default().plugins = vec!["clock".into()];
        d.computers.entry("b".into()).or_default().plugins = vec!["dock".into()];
        d.upgrade();
        assert_eq!(d.plugins, vec!["clock".to_string(), "dock".to_string()]);
        // Switched off later, it stays off.
        toggle(&mut d.plugins, "dock", false);
        d.upgrade();
        assert_eq!(d.plugins, vec!["clock".to_string()]);
    }

    #[test]
    fn upgrade_keeps_plugins_syncing_once() {
        let mut d = Data {
            folders: vec![".config/omarchy".into()],
            ..Default::default()
        };
        d.upgrade();
        assert!(NESTED[..2].iter().all(|f| d.folders.iter().any(|x| x == f)));
        // Turned off after the upgrade, they stay off.
        toggle(&mut d.folders, NESTED[0], false);
        d.upgrade();
        assert!(!d.folders.iter().any(|x| x == NESTED[0]));
        let mut fresh = Data::default();
        fresh.upgrade();
        assert!(fresh.folders.is_empty());
    }

    #[test]
    fn apps_respect_per_computer_skips() {
        let mut d = Data {
            apps: vec!["steam".into(), "spotify".into()],
            ..Default::default()
        };
        d.set_app("laptop", "steam", false);
        assert!(!d.wants_app("laptop", "steam"));
        assert!(d.wants_app("laptop", "spotify"));
        assert!(d.wants_app("desktop", "steam"));
        assert!(!d.wants_app("desktop", "zoom"));
        d.set_app("laptop", "steam", true);
        assert!(d.wants_app("laptop", "steam"));
    }

    #[test]
    fn folders_can_be_kept_separate_per_computer() {
        let mut d = Data {
            folders: vec![".config/hypr".into(), ".config/omarchy".into()],
            ..Default::default()
        };
        d.set_folder_here("laptop", ".config/hypr", false);
        assert_eq!(
            d.active_folders("laptop"),
            vec![".config/omarchy".to_string()]
        );
        assert_eq!(d.active_folders("desktop").len(), 2);
    }

    #[test]
    fn leaving_out_a_folder_leaves_out_what_is_synced_inside_it() {
        let mut d = Data {
            folders: FOLDERS.iter().map(|(f, _, _)| f.to_string()).collect(),
            plugins: vec!["dock".into()],
            ..Default::default()
        };
        d.set_folder_here("laptop", ".config/omarchy", false);
        assert!(
            !d.active_folders("laptop")
                .iter()
                .any(|f| f.starts_with(".config/omarchy")),
            "{:?}",
            d.active_folders("laptop")
        );
        assert!(!d.uses_theme("laptop") && d.uses_theme("desktop"));
        assert!(!d.wants_plugin("laptop", "dock") && d.wants_plugin("desktop", "dock"));
        assert!(d.uses_folder("laptop", ".config/hypr/bindings.lua"));
    }

    #[test]
    fn data_round_trips_through_yaml_the_ignore_template_reads() {
        let mut d = Data {
            apps: vec!["steam".into()],
            ..Default::default()
        };
        d.set_app("laptop", "steam", false);
        d.computers.entry("desktop".into()).or_default();
        let yaml = serde_yaml::to_string(&d).unwrap();
        assert!(yaml.contains("desktop: {}"), "{yaml}");
        assert_eq!(serde_yaml::from_str::<Data>(&yaml).unwrap(), d);
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
    fn history_reads_git_log_in_plain_words() {
        assert_eq!(
            source_to_target("private_dot_config/omarchy/private_shell.json"),
            ".config/omarchy/shell.json"
        );
        assert_eq!(
            source_to_target("dot_local/share/applications/x.desktop"),
            ".local/share/applications/x.desktop"
        );
        let log = "\x1eaaa\tlaptop\t200\tUndo: a change from desk\tbbb\n\nM\tdot_config/hypr/bindings.lua\nM\t.chezmoidata.yaml\n\x1ebbb\tdesk\t100\tSync from desk\t\n\nA\tdot_config/hypr/input.lua\nD\tdot_config/hypr/bindings.lua\n";
        let h = parse_history(log);
        assert_eq!(h.len(), 2);
        assert_eq!(
            h[0].labels,
            vec!["Keyboard shortcuts".to_string(), DATA_LABEL.to_string()]
        );
        assert!(h[0].is_undo && !h[0].first);
        assert_eq!(
            (
                h[1].computer.as_str(),
                h[1].when,
                h[1].first,
                h[1].labels.len()
            ),
            ("desk", 100, true, 2)
        );
        assert_eq!(
            h[1].files,
            vec![
                Change {
                    path: ".config/hypr/input.lua".into(),
                    how: "Added"
                },
                Change {
                    path: ".config/hypr/bindings.lua".into(),
                    how: "Removed"
                },
            ]
        );
        assert_eq!(
            describe(&h[0].files),
            format!("Keyboard shortcuts: changed bindings.lua\n{DATA_LABEL}: updated")
        );
    }

    #[test]
    fn computers_are_identified_by_machine_not_hostname() {
        // Two machines both kept Omarchy's default hostname under the old, hostname-keyed version.
        let mut d = Data::default();
        d.computers.insert(
            "omarchy".into(),
            Computer {
                skip_apps: vec!["steam".into()],
                ..Default::default()
            },
        );
        d.adopt_legacy("aaaa11112222", "omarchy");
        assert!(!d.computers.contains_key("omarchy"));
        let first = &d.computers["aaaa11112222"];
        assert_eq!((first.name.as_str(), first.skip_apps.len()), ("omarchy", 1));
        // Old commits authored "omarchy" now belong to that machine.
        assert_eq!(d.resolve("omarchy"), "aaaa11112222");
        assert_eq!(d.resolve("someone-else"), "someone-else");
        // The second machine gets its own entry and a name that doesn't clash.
        d.adopt_legacy("bbbb33334444", "omarchy");
        assert!(!d.computers.contains_key("bbbb33334444"));
        assert_eq!(d.fresh_name("bbbb33334444", "omarchy"), "omarchy 2");
        assert_eq!(d.fresh_name("aaaa11112222", "omarchy"), "omarchy");
        assert_eq!(d.fresh_name("cccc", ""), "Computer");
    }

    #[test]
    fn missing_apps_only_counts_wanted_and_absent() {
        let mut d = Data {
            apps: vec!["a".into(), "b".into(), "c".into()],
            ..Default::default()
        };
        d.set_app("pc", "c", false);
        let installed: BTreeSet<String> = ["a".to_string()].into();
        assert_eq!(missing_apps(&d, "pc", &installed), vec!["b".to_string()]);
    }

    #[test]
    fn user_apps_skip_omarchy_and_system() {
        let base: BTreeSet<String> = ["hyprland".to_string()].into();
        let got = user_apps(
            ["hyprland", "omarchy-keyring", "sudo", "brave-bin"].into_iter(),
            &base,
        );
        assert_eq!(got, vec!["brave-bin"]);
    }

    #[test]
    fn reads_git_transfer_percentages() {
        assert_eq!(
            transfer_percent("Receiving objects:  45% (9/20), 1.2 MiB | 3 MiB/s"),
            Some(45)
        );
        assert_eq!(
            transfer_percent("Writing objects: 100% (3/3), done."),
            Some(100)
        );
        assert_eq!(transfer_percent("Compressing objects:  50% (1/2)"), None);
        assert_eq!(
            transfer_percent("remote: Enumerating objects: 5, done."),
            None
        );
    }

    #[test]
    fn an_older_omarchy_pauses_its_own_settings_until_tried_or_updated() {
        let on = |v: &str| Computer {
            omarchy: v.into(),
            ..Default::default()
        };
        let mut d = Data {
            folders: vec![".config/hypr".into(), ".config/alacritty".into()],
            ..Default::default()
        };
        d.computers.insert("old".into(), on("4.0.6-1"));
        d.computers.insert("new".into(), on("5.1.0"));
        d.computers.insert("dev".into(), on("dev (abc123)"));
        assert_eq!((major("4.0.6-1"), major("dev (abc123)")), (Some(4), None));
        // The newest computers carry on; an older one pauses, and so would a 4.5 next to a 4.0
        // only if the major version differed.
        assert!(d.should_pause("old") && !d.should_pause("new") && !d.should_pause("dev"));
        d.computers.get_mut("old").unwrap().paused = true;
        assert_eq!(d.active_folders("old"), [".config/alacritty"]);
        assert_eq!(d.active_folders("new").len(), 2);
        // Trying Omarchy 5's settings resumes it, until an Omarchy 6 arrives.
        d.computers.get_mut("old").unwrap().tried = 5;
        assert!(!d.should_pause("old"));
        d.computers.get_mut("new").unwrap().omarchy = "6.0.0".into();
        assert!(d.should_pause("old"));
    }

    #[test]
    fn apps_from_an_extra_repository_wait_for_it() {
        let mut d = Data {
            apps: vec!["chatgpt-bin".into(), "firefox".into()],
            ..Default::default()
        };
        d.app_repo
            .insert("chatgpt-bin".into(), "openai-chatgpt".into());
        d.repositories.insert(
            "openai-chatgpt".into(),
            Repository {
                servers: vec!["https://example.com/arch".into()],
                ..Default::default()
            },
        );
        let none = BTreeSet::new();
        let missing = missing_repos(&d, "b", &none, &BTreeSet::from(["core".into()]));
        assert_eq!(
            (
                missing.len(),
                missing[0].name.as_str(),
                missing[0].apps.clone()
            ),
            (1, "openai-chatgpt", vec!["chatgpt-bin".to_string()])
        );
        // Once the repository is here, nothing waits for it.
        let here = BTreeSet::from(["openai-chatgpt".to_string()]);
        assert!(missing_repos(&d, "b", &none, &here).is_empty());
        assert_eq!(repos_with_apps(&d, &here)[0].apps, missing[0].apps);
        // A repository recorded on one computer survives a merge with another's changes.
        let base = Data::default();
        let theirs = Data {
            apps: vec!["yazi".into()],
            ..Default::default()
        };
        let m = Data::merge(&base, &theirs, &d);
        assert!(m.repositories.contains_key("openai-chatgpt"));
        assert_eq!(
            m.app_repo.get("chatgpt-bin").map(String::as_str),
            Some("openai-chatgpt")
        );
    }

    #[test]
    fn a_tampered_repository_is_refused_before_anything_runs() {
        let mut d = Data::default();
        let repo = |server: &str, level: &str| Repository {
            servers: vec![server.into()],
            sig_level: vec![level.into()],
            key: None,
        };
        d.repositories
            .insert("plain-http".into(), repo("http://example.com", "Required"));
        d.repositories.insert(
            "odd-level".into(),
            repo("https://example.com", "Required; rm -rf"),
        );
        d.repositories
            .insert("bad name!".into(), repo("https://example.com", "Required"));
        d.repositories
            .insert("core".into(), repo("https://example.com", "Required"));
        for name in ["plain-http", "odd-level", "bad name!", "core", "unknown"] {
            assert!(
                add_repository(&d, name).is_err(),
                "{name} should be refused"
            );
        }
    }

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
    fn friendly_picks_the_sentence_by_cause() {
        let verify = "attestation: gh attestation verify x: To get started with GitHub CLI, please run: gh auth login";
        assert!(friendly(verify).starts_with("Not signed in to GitHub"));
        assert!(friendly("checksum: sha256sum -c: x: FAILED").starts_with("The downloaded update"));
        assert!(friendly("0.1.2 isn't the latest release").starts_with("A newer release appeared"));
        assert!(friendly("curl: Could not resolve host").starts_with("Couldn't reach GitHub"));
    }

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
