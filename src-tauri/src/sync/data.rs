//! The shared choices file (`.chezmoidata.yaml`): its shape, how two versions merge, and how older layouts are upgraded.

use super::*;

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
    /// The git address of each plugin that came from git (`omarchy plugin add`). Such a plugin
    /// isn't copied file by file: a computer that takes it adds it from git too, so every copy
    /// can be updated.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugin_sources: BTreeMap<String, String>,
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
            computers: merge_map(&base.computers, &theirs.computers, &mine.computers),
            outside: merge_map(&base.outside, &theirs.outside, &mine.outside),
            repositories: merge_map(&base.repositories, &theirs.repositories, &mine.repositories),
            app_repo: merge_map(&base.app_repo, &theirs.app_repo, &mine.app_repo),
            plugin_sources: merge_map(
                &base.plugin_sources,
                &theirs.plugin_sources,
                &mine.plugin_sources,
            ),
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
}

/// A plain file name, safe to write into the ignore rules: no folders, wildcards or rule syntax.
pub(crate) fn is_file_name(n: &str) -> bool {
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

/// A plugin as people see it: its manifest's name and description, and how Omarchy sees it here.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    pub description: String,
    /// What it adds: bar-widget, panel, overlay, service, bar.
    pub kinds: Vec<String>,
    /// Part of Omarchy itself (only ever on or off), not added by anyone.
    pub first_party: bool,
    /// Whether it's on this computer.
    pub here: bool,
    /// Switched on in the Omarchy shell here (None when Omarchy doesn't know it here).
    pub enabled: Option<bool>,
    /// The active bar can't be switched off.
    pub can_disable: bool,
    /// Its git address, when it came from git (so it can be updated).
    pub source: Option<String>,
}

/// Adds or removes `item` so that `list.contains(item) == present`.
pub fn toggle(list: &mut Vec<String>, item: &str, present: bool) {
    list.retain(|x| x != item);
    if present {
        list.push(item.into());
    }
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
}
