//! Which computer gets what: the per-computer switches on the shared choices.

use super::*;

impl Data {
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

    /// Leaves an app out on this computer only (or takes it back), like `set_folder_here`.
    pub fn set_app_here(&mut self, host: &str, app: &str, on: bool) {
        toggle(
            &mut self.computers.entry(host.into()).or_default().skip_apps,
            app,
            !on,
        );
    }

    /// The app's one switch: on syncs it to every computer, this one included; off stops it
    /// everywhere, and no computer's skip is left behind.
    pub fn set_app_all(&mut self, host: &str, app: &str, on: bool) {
        toggle(&mut self.apps, app, on);
        if on {
            self.set_app_here(host, app, true);
        } else {
            for c in self.computers.values_mut() {
                toggle(&mut c.skip_apps, app, false);
            }
        }
    }

    pub fn set_folder_here(&mut self, host: &str, folder: &str, on: bool) {
        toggle(
            &mut self.computers.entry(host.into()).or_default().skip_files,
            folder,
            !on,
        );
    }

    pub fn wants_plugin(&self, host: &str, plugin: &str) -> bool {
        self.plugins.iter().any(|p| p == plugin)
            && self.uses_folder(host, PLUGIN_DIR)
            && !self
                .computers
                .get(host)
                .is_some_and(|c| c.skip_plugins.iter().any(|p| p == plugin))
    }

    /// Leaves a synced plugin out on this computer only (or takes it back).
    pub fn set_plugin_here(&mut self, host: &str, plugin: &str, on: bool) {
        toggle(
            &mut self.computers.entry(host.into()).or_default().skip_plugins,
            plugin,
            !on,
        );
    }

    /// The plugin's one switch, as `set_app_all`.
    pub fn set_plugin_all(&mut self, host: &str, plugin: &str, on: bool) {
        toggle(&mut self.plugins, plugin, on);
        if on {
            // Synced again: computers that haven't removed their copy yet keep it.
            self.removed_plugins.remove(plugin);
            self.set_plugin_here(host, plugin, true);
        } else {
            for c in self.computers.values_mut() {
                toggle(&mut c.skip_plugins, plugin, false);
            }
        }
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
        // Synced again: computers that haven't removed their copy yet keep it.
        if on {
            self.removed_plugins.remove(plugin);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

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
        d.set_plugin_here("a", "dock", false);
        assert!(d.missing_plugins("a").is_empty());
        d.set_plugin_here("a", "dock", true);
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
    fn one_switch_reaches_every_computer_and_here_leaves_one_out() {
        let mut d = Data::default();
        d.set_app_all("here", "zed", true);
        d.set_app_here("lap", "zed", false);
        assert!(d.wants_app("here", "zed") && d.wants_app("desk", "zed"));
        assert!(!d.wants_app("lap", "zed"));
        // Off everywhere leaves no skip behind, so on again reaches Lap too.
        d.set_app_all("here", "zed", false);
        assert!(d.apps.is_empty() && d.computers.values().all(|c| c.skip_apps.is_empty()));
        d.set_app_all("here", "zed", true);
        assert!(d.wants_app("lap", "zed"));
        // Turning it on here clears this computer's own skip.
        d.set_app_here("here", "zed", false);
        d.set_app_all("here", "zed", true);
        assert!(d.wants_app("here", "zed"));

        d.removed_plugins.insert("dock".into(), vec!["lap".into()]);
        d.set_plugin_all("here", "dock", true);
        assert!(d.removed_plugins.is_empty() && d.wants_plugin("lap", "dock"));
        d.set_plugin_here("lap", "dock", false);
        assert!(!d.wants_plugin("lap", "dock") && d.wants_plugin("here", "dock"));
        d.set_plugin_all("here", "dock", false);
        assert!(d.plugins.is_empty() && d.computers.values().all(|c| c.skip_plugins.is_empty()));
    }

    #[test]
    fn apps_respect_per_computer_skips() {
        let mut d = Data {
            apps: vec!["steam".into(), "spotify".into()],
            ..Default::default()
        };
        d.set_app_here("laptop", "steam", false);
        assert!(!d.wants_app("laptop", "steam"));
        assert!(d.wants_app("laptop", "spotify"));
        assert!(d.wants_app("desktop", "steam"));
        assert!(!d.wants_app("desktop", "zoom"));
        d.set_app_here("laptop", "steam", true);
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
}
