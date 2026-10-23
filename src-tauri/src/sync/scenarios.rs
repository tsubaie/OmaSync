//! Sync scenarios, each a short story told with the lab (see lab.rs). They need chezmoi and
//! git, so they only run with `cargo test -- --include-ignored`, which `pnpm audit:rust` does.

use super::lab::Lab;
use super::*;

/// A first computer with a background, a web app and a plugin, and a second one joining.
fn pair() -> Lab {
    let mut lab = Lab::new();
    lab.add("a");
    lab.write(
        "a",
        ".config/alacritty/alacritty.toml",
        "a = 1\n\nb = 2\n\nc = 3\n",
    );
    lab.write(
        "a",
        ".config/omarchy/plugins/dock/manifest.json",
        r#"{"name":"Dock"}"#,
    );
    lab.write(
        "a",
        ".local/share/applications/Chat App.desktop",
        "[Desktop Entry]\nExec=omarchy-launch-webapp https://chat.example\nIcon=chat\n",
    );
    lab.write(
        "a",
        ".local/share/applications/mpv.desktop",
        "[Desktop Entry]\nExec=mpv\n",
    );
    lab.write("a", ".local/share/applications/mimeinfo.cache", "cache");
    lab.write(
        "a",
        ".local/share/icons/hicolor/256x256/apps/chat.png",
        "png",
    );
    lab.write(
        "a",
        ".local/share/icons/hicolor/256x256/apps/other.png",
        "png",
    );
    lab.write(
        "a",
        ".config/omarchy/themes/old/colors.toml",
        "accent = 0\n",
    );
    lab.write("a", "Pictures/hills.jpg", "jpg");
    let state = lab.ctx("a").home.join(".local/state/omarchy/current");
    std::os::unix::fs::symlink(
        lab.ctx("a").home.join("Pictures/hills.jpg"),
        state.join("background"),
    )
    .unwrap();
    lab.start("a");
    lab.add("b");
    lab
}

fn status(lab: &Lab, name: &str) -> String {
    let text = std::fs::read_to_string(state_file(lab.ctx(name)).with_file_name("status.json"))
        .unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&text).unwrap_or_default()["state"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
#[ignore]
fn a_second_computer_gets_the_settings_but_keeps_its_own_screens() {
    let mut lab = pair();
    assert!(lab.ctx("a").src.join(".background/hills.jpg").exists());
    assert!(
        !lab.ctx("a")
            .src
            .join("dot_config/omarchy/backgrounds")
            .exists()
    );
    // A theme picked in Omarchy shows as not shared yet, before any sync records it.
    let a = lab.ctx("a").clone();
    assert!(look_changes(&a, &a.load_data()).is_empty());
    lab.write("a", ".local/state/omarchy/current/theme.name", "old\n");
    assert_eq!(
        look_changes(&a, &a.load_data()),
        [".config/omarchy/themes/old"]
    );
    lab.write("a", ".local/state/omarchy/current/theme.name", "mine\n");

    assert!(lab.join("b").exists());
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "-- a keys\n");
    assert_eq!(lab.read("b", ".config/hypr/monitors.lua"), "-- b screen\n");
    assert_eq!(lab.data("b").background.as_deref(), Some("hills.jpg"));
    assert!(
        lab.calls("b").contains("theme bg set"),
        "{}",
        lab.calls("b")
    );
    // Only the current theme's folder travels.
    assert!(lab.exists("b", ".config/omarchy/themes/mine/colors.toml"));
    assert!(!lab.exists("b", ".config/omarchy/themes/old"));
    // Only the web app and its icon travel; other shortcuts, caches and icons don't.
    assert!(lab.exists("b", ".local/share/applications/Chat App.desktop"));
    assert!(lab.exists("b", ".local/share/icons/hicolor/256x256/apps/chat.png"));
    assert!(!lab.exists("b", ".local/share/applications/mpv.desktop"));
    assert!(!lab.exists("b", ".local/share/applications/mimeinfo.cache"));
    assert!(!lab.exists("b", ".local/share/icons/hicolor/256x256/apps/other.png"));
}

#[test]
#[ignore]
fn a_plugin_waits_until_taken_and_an_unshared_one_stays_put() {
    let mut lab = pair();
    lab.join("b");
    // A's plugin waits on B until B takes it; the bar shows it waiting.
    assert_eq!(status(&lab, "b"), "attention");
    assert!(!lab.exists("b", ".config/omarchy/plugins/dock/manifest.json"));
    assert_eq!(lab.data("b").missing_plugins("b"), vec!["dock".to_string()]);
    assert_eq!(lab.ctx("b").plugin("dock").name, "Dock");
    install_plugins(lab.ctx("b"), &["dock".into()]).unwrap();
    assert!(lab.exists("b", ".config/omarchy/plugins/dock/manifest.json"));
    assert!(lab.data("b").missing_plugins("b").is_empty());
    lab.sync_ok("b");
    assert_eq!(status(&lab, "b"), "synced");

    // A plugin nobody switched on stays on the computer that has it.
    lab.write("a", ".config/omarchy/plugins/solo/manifest.json", "{}");
    lab.settle();
    assert!(!lab.exists("b", ".config/omarchy/plugins/solo"));
    assert!(
        !lab.ctx("b")
            .src
            .join("dot_config/omarchy/plugins/solo")
            .exists()
    );
    assert!(lab.data("b").missing_plugins("b").is_empty());
}

#[test]
#[ignore]
fn the_same_file_changed_on_both_asks_and_keeps_both_versions() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/hypr/bindings.lua", "a2");
    lab.sync_ok("a");
    lab.write("b", ".config/hypr/bindings.lua", "b2");
    let ov = overview(lab.ctx("b"), &|_| {}, false).unwrap();
    assert_eq!(
        (ov.incoming, ov.unshared.clone()),
        (1, vec![".config/hypr/bindings.lua".to_string()])
    );
    let clash = lab.clash("b");
    assert_eq!(clash.files, vec![".config/hypr/bindings.lua"]);
    assert_eq!(clash.by.as_ref().map(|(name, _)| name.as_str()), Some("a"));
    let v = &clash.versions[0];
    assert_eq!(
        (v.mine.as_deref(), v.theirs.as_deref(), v.binary),
        (Some("b2"), Some("a2"), false)
    );
    // Asking changes nothing.
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "b2");

    lab.resolve("b", &clash, |_| Keep::Mine).unwrap();
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "b2");
    lab.sync_ok("a");
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "b2");
    assert_eq!(lab.read("a", ".config/hypr/monitors.lua"), "-- a screen\n");
    let ov = overview(lab.ctx("a"), &|_| {}, false).unwrap();
    assert_eq!(
        (ov.incoming, ov.unshared.len()),
        (0, 0),
        "{:?}",
        ov.unshared
    );
    assert_eq!(
        ov.computers
            .iter()
            .map(|c| c.0.as_str())
            .collect::<Vec<_>>(),
        vec!["a", "b"]
    );
    let last = &ov.history[0];
    assert!(last.is_merge && !last.first, "{last:?}");
    assert!(lab.history("b").iter().any(|e| e.is_merge));

    // The other way round: B keeps A's, and B's own version stays in the history.
    lab.write("a", ".config/hypr/bindings.lua", "a4");
    lab.sync_ok("a");
    lab.write("b", ".config/hypr/bindings.lua", "b4");
    let clash = lab.clash("b");
    lab.resolve("b", &clash, |_| Keep::Theirs).unwrap();
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "a4");
    let kept = lab.ctx("b").git(&["log", "-p"]).unwrap_or_default();
    assert!(kept.contains("+b4"), "B's own version is in the history");
    lab.settle();
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "a4");
}

#[test]
#[ignore]
fn undo_reaches_the_other_computer() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/hypr/bindings.lua", "a3");
    lab.sync_ok("a");
    let last = &lab.history("a")[0];
    assert!(
        !last.first && last.labels.iter().any(|l| l == "Keyboard shortcuts"),
        "{last:?}"
    );
    lab.undo("a", 0).unwrap();
    lab.sync_ok("a");
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "-- a keys\n");
    assert!(lab.history("a")[0].is_undo);
    lab.sync_ok("b");
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "-- a keys\n");
    assert!(lab.history("b").last().unwrap().first);
}

#[test]
#[ignore]
fn a_file_that_combined_into_nonsense_is_a_clash() {
    // Both add the same setting in different parts of a TOML file: git combines them without
    // a clash, but the result has it twice, which isn't valid TOML. B goes back to where it
    // was and is asked, with the file marked as broken by combining.
    let mut lab = pair();
    lab.join("b");
    let toml = ".config/alacritty/alacritty.toml";
    lab.write("a", toml, "a = 1\nx = \"a\"\n\nb = 2\n\nc = 3\n");
    lab.sync_ok("a");
    let mine = "a = 1\n\nb = 2\n\nc = 3\nx = \"b\"\n";
    lab.write("b", toml, mine);
    let clash = lab.clash("b");
    assert_eq!(clash.broken, vec![toml]);
    assert_eq!(lab.read("b", toml), mine);
    lab.resolve("b", &clash, |_| Keep::Mine).unwrap();
    assert_eq!(lab.read("b", toml), mine);
    lab.settle();
    assert_eq!(lab.read("a", toml), mine);
}

// ---------- More computers, switches and strange habits ----------

#[test]
#[ignore]
fn three_computers_end_up_the_same() {
    let mut lab = pair();
    lab.join("b");
    lab.add("c");
    lab.join("c");
    lab.write("a", ".config/hypr/input.lua", "from a\n");
    lab.write("b", ".config/hypr/looknfeel.lua", "from b\n");
    lab.write("c", ".config/hypr/autostart.lua", "from c\n");
    lab.settle();
    for n in ["a", "b", "c"] {
        assert_eq!(lab.read(n, ".config/hypr/input.lua"), "from a\n");
        assert_eq!(lab.read(n, ".config/hypr/looknfeel.lua"), "from b\n");
        assert_eq!(lab.read(n, ".config/hypr/autostart.lua"), "from c\n");
        assert_eq!(
            lab.read(n, ".config/hypr/monitors.lua"),
            format!("-- {n} screen\n")
        );
    }
    let ov = overview(lab.ctx("c"), &|_| {}, false).unwrap();
    let mut listed: Vec<&str> = ov.computers.iter().map(|c| c.0.as_str()).collect();
    listed.sort();
    assert_eq!(listed, ["a", "b", "c"]);
}

#[test]
#[ignore]
fn deleting_a_file_deletes_it_nowhere_else() {
    let mut lab = pair();
    lab.join("b");
    lab.remove("a", ".config/hypr/bindings.lua");
    lab.settle();
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "-- a keys\n");
    // Deletions don't travel, and the deleted file is only ever itself when it comes back.
    if lab.exists("a", ".config/hypr/bindings.lua") {
        assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "-- a keys\n");
    }
}

#[test]
#[ignore]
fn a_folder_switched_off_leaves_every_computer_with_its_own_version() {
    let mut lab = pair();
    lab.join("b");
    let toml = ".config/alacritty/alacritty.toml";
    lab.set_folder("a", ".config/alacritty", false).unwrap();
    lab.write("a", toml, "a = 1\n");
    lab.write("b", toml, "b = 1\n");
    lab.settle();
    assert_eq!(lab.read("a", toml), "a = 1\n");
    assert_eq!(lab.read("b", toml), "b = 1\n");
    assert!(
        !lab.data("b")
            .folders
            .iter()
            .any(|f| f == ".config/alacritty")
    );

    // Switched back on: the computer that switched it shares its version, and the other's
    // own version is saved before it goes.
    lab.set_folder("a", ".config/alacritty", true).unwrap();
    lab.sync_ok("a");
    let report = lab.sync_ok("b");
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("Alacritty terminal now sync")),
        "{report:?}"
    );
    assert_eq!(lab.read("b", toml), "a = 1\n");
    lab.settle();
}

#[test]
#[ignore]
fn dont_sync_on_this_computer_leaves_only_it_out() {
    let mut lab = pair();
    lab.join("b");
    lab.add("c");
    lab.join("c");
    let toml = ".config/alacritty/alacritty.toml";
    lab.edit("c", |d, host| {
        d.set_folder_here(host, ".config/alacritty", false)
    });
    lab.sync_ok("c");
    lab.write("c", toml, "c only\n");
    lab.write("a", toml, "a = 2\n");
    lab.settle();
    assert_eq!(lab.read("b", toml), "a = 2\n");
    assert_eq!(lab.read("c", toml), "c only\n");
    // Back in: c's own version is kept somewhere before the shared one replaces it.
    lab.edit("c", |d, host| {
        d.set_folder_here(host, ".config/alacritty", true)
    });
    match lab.sync("c") {
        Ok(_) => {}
        Err(SyncError::Conflict(c)) => {
            lab.resolve("c", &c, |_| Keep::Theirs).unwrap();
        }
        Err(SyncError::Failed(e)) => panic!("{e}"),
    }
    lab.settle();
    let all: BTreeSet<String> = ["a", "b", "c"].iter().map(|n| lab.read(n, toml)).collect();
    assert_eq!(all.len(), 1, "{all:?}");
}

#[test]
#[ignore]
fn a_nested_switch_off_keeps_the_rest_of_the_folder_syncing() {
    let mut lab = pair();
    lab.join("b");
    lab.set_folder("a", ".config/hypr/bindings.lua", false)
        .unwrap();
    lab.write("a", ".config/hypr/bindings.lua", "a keys\n");
    lab.write("b", ".config/hypr/bindings.lua", "b keys\n");
    lab.write("a", ".config/hypr/input.lua", "shared input\n");
    lab.settle();
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "a keys\n");
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "b keys\n");
    assert_eq!(lab.read("b", ".config/hypr/input.lua"), "shared input\n");
}

#[test]
#[ignore]
fn both_change_the_choices_at_once_and_both_choices_stay() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/nvim/init.lua", "vim\n");
    lab.write("b", ".config/btop/btop.conf", "top\n");
    lab.set_folder("a", ".config/nvim", true).unwrap();
    lab.set_folder("b", ".config/btop", true).unwrap();
    lab.sync_ok("a");
    lab.sync_ok("b");
    lab.settle();
    let folders = lab.data("a").folders;
    assert!(folders.contains(&".config/nvim".to_string()), "{folders:?}");
    assert!(folders.contains(&".config/btop".to_string()), "{folders:?}");
    assert_eq!(lab.read("b", ".config/nvim/init.lua"), "vim\n");
    assert_eq!(lab.read("a", ".config/btop/btop.conf"), "top\n");
}

#[test]
#[ignore]
fn a_clash_on_several_files_takes_a_choice_per_file() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/hypr/bindings.lua", "a keys\n");
    lab.write("a", ".config/hypr/input.lua", "a input\n");
    lab.sync_ok("a");
    lab.write("b", ".config/hypr/bindings.lua", "b keys\n");
    lab.write("b", ".config/hypr/input.lua", "b input\n");
    let clash = lab.clash("b");
    assert_eq!(clash.files.len(), 2);
    lab.resolve("b", &clash, |path| {
        if path.ends_with("bindings.lua") {
            Keep::Mine
        } else {
            Keep::Theirs
        }
    })
    .unwrap();
    lab.settle();
    for n in ["a", "b"] {
        assert_eq!(lab.read(n, ".config/hypr/bindings.lua"), "b keys\n");
        assert_eq!(lab.read(n, ".config/hypr/input.lua"), "a input\n");
    }
}

#[test]
#[ignore]
fn an_undo_and_a_change_to_the_same_file_meet_as_a_clash() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/hypr/bindings.lua", "v2\n");
    lab.sync_ok("a");
    lab.sync_ok("b");
    lab.undo("a", 0).unwrap();
    lab.sync_ok("a");
    lab.write("b", ".config/hypr/bindings.lua", "v3\n");
    let clash = lab.clash("b");
    assert_eq!(clash.files, vec![".config/hypr/bindings.lua"]);
    lab.resolve("b", &clash, |_| Keep::Mine).unwrap();
    lab.settle();
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "v3\n");
}

#[test]
#[ignore]
fn undoing_an_undo_and_an_undo_that_later_changes_block() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/hypr/bindings.lua", "v2\n");
    lab.sync_ok("a");
    lab.undo("a", 0).unwrap();
    lab.sync_ok("a");
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "-- a keys\n");
    lab.undo("a", 0).unwrap();
    lab.sync_ok("a");
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "v2\n");
    // A later change to the same file builds on this one.
    lab.write("a", ".config/hypr/bindings.lua", "v4\n");
    lab.sync_ok("a");
    let history = lab.history("a");
    let plain: Vec<usize> = history
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.first && !e.is_undo && !e.is_merge)
        .map(|(i, _)| i)
        .collect();
    assert!(lab.undo("a", plain[1]).unwrap_err().contains("can't undo"));
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "v4\n");
    lab.settle();
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "v4\n");
}

#[test]
#[ignore]
fn a_rename_and_a_forgotten_computer_that_comes_back() {
    let mut lab = pair();
    lab.join("b");
    lab.edit("a", |d, host| {
        d.computers.get_mut(host).unwrap().name = "Laptop".into()
    });
    lab.settle();
    assert_eq!(lab.data("b").computers["a"].name, "Laptop");
    lab.edit("a", |d, _| {
        d.computers.remove("b");
    });
    lab.sync_ok("a");
    assert!(!lab.data("a").computers.contains_key("b"));
    lab.settle();
    assert_eq!(lab.data("a").computers["b"].name, "b");
}

#[test]
#[ignore]
fn two_computers_with_the_same_hostname_get_told_apart() {
    let mut lab = Lab::new();
    lab.add_named("id-one", "omarchy");
    lab.add_named("id-two", "omarchy");
    lab.add_named("id-three", "omarchy");
    lab.start("id-one");
    lab.join("id-two");
    lab.join("id-three");
    lab.settle();
    let data = lab.data("id-one");
    let names: Vec<&str> = ["id-one", "id-two", "id-three"]
        .iter()
        .map(|id| data.computers[*id].name.as_str())
        .collect();
    assert_eq!(names, ["omarchy", "omarchy 2", "omarchy 3"]);
}

// ---------- Omarchy versions ----------

#[test]
#[ignore]
fn an_older_omarchy_keeps_its_own_settings_until_it_tries_or_updates() {
    let mut lab = pair();
    lab.omarchy_version("a", "5.0.0-1");
    lab.omarchy_version("b", "4.2.0-1");
    lab.sync_ok("a");
    lab.join("b");
    assert!(lab.data("b").computers["b"].paused);
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "-- b keys\n");
    assert_eq!(
        lab.read("b", ".config/alacritty/alacritty.toml"),
        "a = 1\n\nb = 2\n\nc = 3\n"
    );
    lab.write("a", ".config/hypr/bindings.lua", "omarchy 5 keys\n");
    lab.settle();
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "-- b keys\n");

    // Trying them: b's own are saved first, and Put Back restores them exactly.
    lab.edit("b", |d, host| d.computers.get_mut(host).unwrap().tried = 5);
    let report = lab.sync_ok("b");
    assert!(
        report.notes.iter().any(|n| n.contains("saved first")),
        "{report:?}"
    );
    assert_eq!(
        lab.read("b", ".config/hypr/bindings.lua"),
        "omarchy 5 keys\n"
    );
    put_back_omarchy(lab.ctx("b")).unwrap();
    lab.check("b");
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "-- b keys\n");
    lab.sync_ok("b");
    assert!(lab.data("b").computers["b"].paused);
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "-- b keys\n");

    // Updated: b takes the shared settings for good, with its own saved once more.
    lab.omarchy_version("b", "5.0.1-1");
    lab.sync_ok("b");
    assert!(!lab.data("b").computers["b"].paused);
    assert_eq!(
        lab.read("b", ".config/hypr/bindings.lua"),
        "omarchy 5 keys\n"
    );
    lab.settle();
}

// ---------- Themes, backgrounds and apps ----------

#[test]
#[ignore]
fn a_new_theme_travels_and_the_others_keep_the_themes_they_have() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".local/state/omarchy/current/theme.name", "old\n");
    lab.settle();
    assert_eq!(lab.data("b").theme.as_deref(), Some("old"));
    assert!(
        lab.calls("b").contains("omarchy theme set old"),
        "{}",
        lab.calls("b")
    );
    assert_eq!(
        lab.read("b", ".config/omarchy/themes/old/colors.toml"),
        "accent = 0\n"
    );
    assert_eq!(
        lab.read("b", ".config/omarchy/themes/mine/colors.toml"),
        "accent = 1\n"
    );
    assert!(
        !lab.ctx("b")
            .src
            .join("dot_config/omarchy/themes/mine")
            .exists()
    );
}

#[test]
#[ignore]
fn a_background_that_no_longer_exists_doesnt_stop_the_sync() {
    let mut lab = pair();
    lab.join("b");
    lab.remove("a", "Pictures/hills.jpg");
    lab.settle();
    assert_eq!(lab.data("b").background.as_deref(), Some("hills.jpg"));
}

#[test]
#[ignore]
fn a_synced_app_waits_for_a_yes_and_can_be_declined_per_computer() {
    let mut lab = pair();
    lab.packages("a", "firefox mpv");
    lab.join("b");
    install_plugins(lab.ctx("b"), &["dock".into()]).unwrap();
    lab.edit("a", |d, _| d.apps.push("firefox".into()));
    lab.sync_ok("a");
    let report = lab.sync_ok("b");
    assert_eq!(report.waiting, vec!["firefox"]);
    assert_eq!(status(&lab, "b"), "attention");
    assert!(
        !lab.calls("b").contains("yay"),
        "nothing installs without a yes"
    );
    lab.edit("b", |d, host| d.set_app(host, "firefox", false));
    let report = lab.sync_ok("b");
    assert!(report.waiting.is_empty());
    assert_eq!(status(&lab, "b"), "synced");
    lab.settle();
    assert!(lab.data("a").apps.contains(&"firefox".to_string()));
}

// ---------- Things going wrong ----------

#[test]
#[ignore]
fn a_failed_upload_is_retried_by_the_next_sync() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/hypr/bindings.lua", "unsent\n");
    lab.fail_git(Some("push"));
    let e = lab.sync_fails("a");
    lab.fail_git(None);
    assert!(e.contains("push"), "{e}");
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "unsent\n");
    assert!(
        lab.history("a")[0]
            .labels
            .iter()
            .any(|l| l == "Keyboard shortcuts")
    );
    lab.sync_ok("b");
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "-- a keys\n");
    lab.settle();
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "unsent\n");
}

#[test]
#[ignore]
fn offline_changes_are_kept_and_shared_later() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/hypr/bindings.lua", "offline\n");
    lab.fail_git(Some("fetch"));
    lab.sync_fails("a");
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "offline\n");
    lab.write("b", ".config/hypr/input.lua", "meanwhile\n");
    lab.sync_fails("b");
    lab.fail_git(None);
    lab.settle();
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "offline\n");
    assert_eq!(lab.read("a", ".config/hypr/input.lua"), "meanwhile\n");
}

#[test]
#[ignore]
fn a_sync_interrupted_part_way_starts_afresh_next_time() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/hypr/bindings.lua", "a keys\n");
    lab.sync_ok("a");
    lab.write("b", ".config/hypr/bindings.lua", "b keys\n");
    // As if the power went during "Getting changes from your other computers": this
    // computer's change saved, git stopped half way through replaying it.
    let b = lab.ctx("b").clone();
    save_local(&b, &|_| {}).unwrap();
    b.git(&["fetch"]).unwrap();
    assert!(b.git(&["rebase", "-q", "@{u}"]).is_err());
    assert!(b.rebasing());
    let clash = lab.clash("b");
    assert_eq!(clash.files, vec![".config/hypr/bindings.lua"]);
    assert!(!lab.ctx("b").rebasing());
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "b keys\n");
    lab.resolve("b", &clash, |_| Keep::Mine).unwrap();
    lab.settle();
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "b keys\n");
}

#[test]
#[ignore]
fn a_lock_file_left_behind_doesnt_block_syncing() {
    let mut lab = pair();
    std::fs::write(lab.ctx("a").src.join(".git/omarchy-sync.lock"), "stale").unwrap();
    lab.sync_ok("a");
}

#[test]
#[ignore]
fn a_sync_waits_for_the_one_already_running() {
    let mut lab = pair();
    let ctx = lab.ctx("a").clone();
    let lock = ctx.src.join(".git/omarchy-sync.lock");
    let steps = std::cell::RefCell::new(Vec::new());
    std::thread::scope(|s| {
        let held = std::fs::File::create(&lock).unwrap();
        held.lock().unwrap();
        s.spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(held);
        });
        sync(&ctx, false, &|step| {
            steps.borrow_mut().push(step.to_string())
        })
        .unwrap();
    });
    lab.check("a");
    assert!(
        steps
            .borrow()
            .iter()
            .any(|s| s == "Waiting for the sync already running"),
        "{:?}",
        steps.borrow()
    );
}

#[test]
#[ignore]
fn disconnecting_leaves_this_computer_as_it_is_and_the_others_syncing() {
    let mut lab = pair();
    lab.join("b");
    lab.add("c");
    lab.join("c");
    lab.write("b", ".config/hypr/bindings.lua", "b before leaving\n");
    lab.disconnect("b");
    assert!(!is_set_up(lab.ctx("b")));
    assert_eq!(
        lab.read("b", ".config/hypr/bindings.lua"),
        "b before leaving\n"
    );
    lab.write("a", ".config/hypr/bindings.lua", "after b left\n");
    lab.sync_ok("a");
    lab.sync_ok("c");
    assert_eq!(lab.read("c", ".config/hypr/bindings.lua"), "after b left\n");
    assert_eq!(
        lab.read("b", ".config/hypr/bindings.lua"),
        "b before leaving\n"
    );
}

// ---------- Files as people really keep them ----------

#[test]
#[ignore]
fn odd_names_modes_and_binary_files_travel_intact() {
    use std::os::unix::fs::PermissionsExt;
    let mut lab = pair();
    lab.join("b");
    let odd = [
        ".config/hypr/my keys (v2).lua",
        ".config/hypr/ünïcödé-ключи.lua",
        ".config/hypr/-starts-with-a-dash.lua",
        ".config/hypr/.hidden.lua",
        ".config/hypr/deep/er/and/deeper/file.lua",
    ];
    for f in odd {
        lab.write("a", f, format!("-- {f}\n"));
    }
    let png: Vec<u8> = (0..=255u8).cycle().take(3000).collect();
    lab.write("a", ".config/hypr/icon.png", &png);
    lab.write("a", ".config/hypr/run.sh", "#!/bin/sh\necho hi\n");
    lab.write("a", ".config/hypr/secret.lua", "token = 'x'\n");
    lab.write("a", ".config/hypr/empty.lua", "");
    let home = lab.ctx("a").home.clone();
    std::fs::set_permissions(
        home.join(".config/hypr/run.sh"),
        PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    std::fs::set_permissions(
        home.join(".config/hypr/secret.lua"),
        PermissionsExt::from_mode(0o600),
    )
    .unwrap();
    lab.settle();
    for f in odd {
        assert_eq!(lab.read("b", f), format!("-- {f}\n"));
    }
    assert_eq!(lab.bytes("b", ".config/hypr/icon.png"), png);
    assert_eq!(lab.read("b", ".config/hypr/empty.lua"), "");
    let mode = |f: &str| {
        std::fs::metadata(lab.ctx("b").home.join(f))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(".config/hypr/run.sh"), 0o755);
    assert_eq!(mode(".config/hypr/secret.lua"), 0o600);

    // A binary file changed on both sides clashes without showing its contents.
    lab.write("a", ".config/hypr/icon.png", b"\0a".as_slice());
    lab.sync_ok("a");
    lab.write("b", ".config/hypr/icon.png", b"\0b".as_slice());
    let clash = lab.clash("b");
    assert!(clash.versions[0].binary && clash.versions[0].mine.is_none());
    lab.resolve("b", &clash, |_| Keep::Theirs).unwrap();
    lab.settle();
    assert_eq!(lab.bytes("b", ".config/hypr/icon.png"), b"\0a");
}

#[test]
#[ignore]
fn caches_git_folders_and_backup_files_never_travel() {
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/omarchy/plugins/dock/.git/config", "[core]\n");
    lab.write(
        "a",
        ".config/omarchy/plugins/dock/node_modules/x/index.js",
        "x",
    );
    lab.write("a", ".config/hypr/bindings.lua.bak.1", "old");
    lab.write("a", ".config/omarchy/backgrounds/big.jpg", "jpg");
    lab.write("a", ".config/hypr/monitors.conf", "monitor=eDP-1");
    install_plugins(lab.ctx("b"), &["dock".into()]).unwrap();
    lab.settle();
    for f in [
        ".config/omarchy/plugins/dock/.git/config",
        ".config/omarchy/plugins/dock/node_modules/x/index.js",
        ".config/hypr/bindings.lua.bak.1",
        ".config/omarchy/backgrounds/big.jpg",
        ".config/hypr/monitors.conf",
    ] {
        assert!(!lab.exists("b", f), "{f} travelled");
    }
}

#[test]
#[ignore]
fn a_settings_file_that_is_a_symlink_arrives_readable() {
    let mut lab = pair();
    lab.join("b");
    lab.write(
        "a",
        "dotfiles/bindings.lua",
        "-- kept in a dotfiles folder\n",
    );
    lab.remove("a", ".config/hypr/bindings.lua");
    let home = lab.ctx("a").home.clone();
    std::os::unix::fs::symlink(
        home.join("dotfiles/bindings.lua"),
        home.join(".config/hypr/bindings.lua"),
    )
    .unwrap();
    lab.settle();
    assert_eq!(
        lab.read("b", ".config/hypr/bindings.lua"),
        "-- kept in a dotfiles folder\n"
    );
}

#[test]
#[ignore]
fn hundreds_of_files_at_once() {
    let mut lab = pair();
    lab.join("b");
    for i in 0..200 {
        lab.write(
            "a",
            &format!(".config/hypr/scripts/{i}.lua"),
            format!("-- {i}\n"),
        );
    }
    lab.settle();
    assert_eq!(lab.read("b", ".config/hypr/scripts/199.lua"), "-- 199\n");
    assert!(lab.history("b").iter().any(|e| e.files.len() >= 200));
}

// ---------- Sync spaces made by earlier versions ----------

/// Where saved sync spaces live, one per layout of the shared choices file.
fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(format!("sync-space-v{}.tar.gz", {
            let mut d = Data::default();
            d.upgrade();
            d.version
        }))
}

/// Saves today's sync space for later versions to test against: run once with
/// `LAB_SAVE_SYNC_SPACE=1` when the layout of the shared choices file changes.
#[test]
#[ignore]
fn save_todays_sync_space() {
    if std::env::var("LAB_SAVE_SYNC_SPACE").is_err() {
        return;
    }
    let mut lab = pair();
    lab.join("b");
    lab.write("a", ".config/hypr/input.lua", "saved for later\n");
    lab.settle();
    std::fs::create_dir_all(fixture().parent().unwrap()).unwrap();
    lab.save_remote(&fixture());
}

#[test]
#[ignore]
fn a_sync_space_made_by_an_earlier_version_still_syncs() {
    let mut lab = Lab::new();
    lab.restore_remote(&fixture());
    lab.add("c");
    lab.join("c");
    assert_eq!(lab.read("c", ".config/hypr/input.lua"), "saved for later\n");
    assert_eq!(lab.read("c", ".config/hypr/monitors.lua"), "-- c screen\n");
    lab.write("c", ".config/hypr/looknfeel.lua", "from a newer version\n");
    lab.sync_ok("c");
    let data = lab.data("c");
    assert!(data.computers.contains_key("a") && data.computers.contains_key("c"));
    assert_eq!(data.theme.as_deref(), Some("mine"));
}

// ---------- Anything, in any order ----------

/// A small deterministic source of random numbers (no crate needed for this).
struct Dice(u64);

impl Dice {
    fn roll(&mut self, sides: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % sides as u64) as usize
    }
}

/// Three computers doing things in a random order: changes, deletions, syncs, undos, switches,
/// and clashes settled either way. The lab's checks hold after every step, and the computers
/// end up the same. `LAB_SEEDS` (how many runs) and `LAB_STEPS` (how long each is) scale it up;
/// a failure names its seed, and `LAB_SEED` replays one.
#[test]
#[ignore]
fn anything_in_any_order_keeps_every_version() {
    let env = |k: &str, d: u64| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let seeds: Vec<u64> = match std::env::var("LAB_SEED") {
        Ok(s) => vec![s.parse().unwrap()],
        Err(_) => (1..=env("LAB_SEEDS", 2)).collect(),
    };
    let steps = env("LAB_STEPS", 12) as usize;
    for seed in seeds {
        let mut log = Vec::new();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            random_run(seed, steps, &mut log)
        }));
        if let Err(e) = outcome {
            eprintln!("seed {seed} failed after:\n{}", log.join("\n"));
            std::panic::resume_unwind(e);
        }
    }
}

fn random_run(seed: u64, steps: usize, log: &mut Vec<String>) {
    const NAMES: [&str; 3] = ["a", "b", "c"];
    const FILES: [&str; 5] = [
        ".config/hypr/bindings.lua",
        ".config/hypr/input.lua",
        ".config/alacritty/alacritty.toml",
        ".config/omarchy/shell.json",
        ".config/hypr/monitors.lua",
    ];
    let mut dice = Dice(seed);
    let mut lab = pair();
    lab.join("b");
    lab.add("c");
    lab.join("c");
    let sync_any =
        |lab: &mut Lab, dice: &mut Dice, log: &mut Vec<String>, who: &str| match lab.sync(who) {
            Ok(_) => {}
            Err(SyncError::Conflict(c)) => {
                assert!(!c.files.is_empty(), "{who}: a clash with nothing to choose");
                let keep = if dice.roll(2) == 0 {
                    Keep::Mine
                } else {
                    Keep::Theirs
                };
                log.push(format!("{who}: clash on {:?}, keeps {keep:?}", c.files));
                lab.resolve(who, &c, |_| keep).unwrap();
            }
            Err(SyncError::Failed(e)) => panic!("{who}: sync failed: {e}"),
        };
    for i in 0..steps {
        let who = NAMES[dice.roll(NAMES.len())];
        let file = FILES[dice.roll(FILES.len())];
        match dice.roll(8) {
            0..=2 => {
                let content = match file.rsplit('.').next() {
                    Some("toml") => format!("n = {i}\n"),
                    Some("json") => format!("{{\"n\": {i}}}\n"),
                    _ => format!("-- step {i} on {who}\n"),
                };
                log.push(format!("{who} writes {file}"));
                lab.write(who, file, content);
            }
            3 | 4 => {
                log.push(format!("{who} syncs"));
                sync_any(&mut lab, &mut dice, log, who);
            }
            5 => {
                let plain = lab
                    .history(who)
                    .iter()
                    .position(|e| !e.first && !e.is_undo && !e.is_merge);
                if let Some(n) = plain {
                    log.push(format!("{who} undoes entry {n}"));
                    let _ = lab.undo(who, n);
                }
            }
            6 => {
                log.push(format!("{who} removes {file}"));
                lab.remove(who, file);
            }
            _ => {
                let folder = [".config/alacritty", ".config/hypr/bindings.lua"][dice.roll(2)];
                let on = !lab.data(who).folders.iter().any(|f| f == folder);
                if on && !lab.exists(who, folder) {
                    continue;
                }
                log.push(format!(
                    "{who} switches {folder} {}",
                    if on { "on" } else { "off" }
                ));
                let _ = lab.set_folder(who, folder, on);
            }
        }
    }
    // Everyone catches up, settling any clash as the dice say.
    for _ in 0..3 {
        for who in NAMES {
            sync_any(&mut lab, &mut dice, log, who);
        }
    }
    lab.settle();
}

// ---------- Found by the random-order run ----------

#[test]
#[ignore]
fn a_clash_across_several_unsent_changes_lists_every_file() {
    let mut lab = pair();
    lab.join("b");
    // Two changes on b that never reached GitHub (no connection), each saved on its own.
    lab.fail_git(Some("push"));
    lab.write("b", ".config/hypr/bindings.lua", "b keys\n");
    lab.sync_fails("b");
    lab.write("b", ".config/hypr/input.lua", "b input\n");
    lab.sync_fails("b");
    lab.fail_git(None);
    lab.write("a", ".config/hypr/bindings.lua", "a keys\n");
    lab.write("a", ".config/hypr/input.lua", "a input\n");
    lab.sync_ok("a");
    let clash = lab.clash("b");
    assert_eq!(
        clash.files,
        vec![".config/hypr/bindings.lua", ".config/hypr/input.lua"]
    );
    lab.resolve("b", &clash, |path| {
        if path.ends_with("input.lua") {
            Keep::Mine
        } else {
            Keep::Theirs
        }
    })
    .unwrap();
    lab.settle();
    assert_eq!(lab.read("a", ".config/hypr/bindings.lua"), "a keys\n");
    assert_eq!(lab.read("a", ".config/hypr/input.lua"), "b input\n");
}

#[test]
#[ignore]
fn undo_never_puts_back_a_file_that_is_switched_off() {
    let mut lab = pair();
    lab.join("b");
    lab.write("b", ".config/hypr/bindings.lua", "b keys\n");
    lab.sync_ok("b");
    lab.set_folder("a", ".config/hypr/bindings.lua", false)
        .unwrap();
    lab.sync_ok("a");
    // b changed the file again meanwhile: its version stays in b's history, the file leaves
    // the sync folder, and b keeps its copy.
    lab.write("b", ".config/hypr/bindings.lua", "b keys 2\n");
    lab.sync_ok("b");
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "b keys 2\n");
    assert!(
        !lab.ctx("b")
            .src
            .join("dot_config/hypr/bindings.lua")
            .exists()
    );
    // Undoing the change that took the file out would put it back on every computer, over
    // copies that were never saved: refused, with nothing changed.
    lab.write("b", ".config/hypr/bindings.lua", "b keys 3, never synced\n");
    let took_out = lab
        .history("b")
        .iter()
        .position(|e| {
            e.files
                .iter()
                .any(|f| f.how == "Removed" && f.path.ends_with("bindings.lua"))
        })
        .unwrap();
    let e = lab.undo("b", took_out).unwrap_err();
    assert!(e.contains("switched off"), "{e}");
    assert_eq!(
        lab.read("b", ".config/hypr/bindings.lua"),
        "b keys 3, never synced\n"
    );
    // Undoing the switch itself is fine: the folder comes back on, and b's copy is saved first.
    let switch = lab
        .history("b")
        .iter()
        .position(|e| e.computer == "a" && e.labels.iter().any(|l| l == DATA_LABEL))
        .unwrap();
    lab.undo("b", switch).unwrap();
    assert!(
        lab.data("b")
            .folders
            .iter()
            .any(|f| f == ".config/hypr/bindings.lua")
    );
    lab.settle();
}

#[test]
#[ignore]
fn a_choices_file_that_cant_be_read_stops_the_sync_instead_of_resetting_everything() {
    let mut lab = pair();
    lab.join("b");
    let before = lab.data("a");
    let saves = lab.history("a").len();
    // A hand edit with a typo (or half a file): the sync refuses, nothing is committed.
    let path = lab.ctx("a").data_path();
    let good = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, "folders: [\n  oops").unwrap();
    let e = lab.sync_fails("a");
    assert!(e.contains("can't be read"), "{e}");
    assert!(friendly(&e).contains("sync choices"));
    assert_eq!(lab.history("a").len(), saves);
    // Put right, it syncs again with every choice still there, here and on b.
    std::fs::write(&path, good).unwrap();
    lab.sync_ok("a");
    lab.settle();
    assert_eq!(lab.data("b").folders, before.folders);
    assert_eq!(lab.data("b").computers.len(), 2);
}

#[test]
#[ignore]
fn a_switch_flipped_during_a_sync_waits_for_it_instead_of_being_lost() {
    let mut lab = pair();
    let ctx = lab.ctx("a").clone();
    // The background timer is mid-sync (it holds the lock); the person flips a switch.
    let lock = ctx.src.join(".git/omarchy-sync.lock");
    let held = std::fs::File::create(&lock).unwrap();
    held.lock().unwrap();
    let started = std::time::Instant::now();
    std::thread::scope(|s| {
        s.spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(held);
        });
        edit(&ctx, |d, host| {
            d.set_folder_here(host, ".config/alacritty", false)
        })
        .unwrap();
    });
    assert!(started.elapsed() >= std::time::Duration::from_millis(300));
    assert!(!lab.data("a").uses_folder("a", ".config/alacritty"));
    lab.sync_ok("a");
    assert!(!lab.data("a").uses_folder("a", ".config/alacritty"));
}

#[test]
#[ignore]
fn leaving_omarchys_settings_out_on_one_computer_still_syncs_the_rest_there() {
    let mut lab = pair();
    lab.join("b");
    // "Don't sync on this computer" for Other Omarchy settings: themes, hooks and plugins
    // inside it stay out too, and syncing goes on for everything else.
    lab.edit("b", |d, host| {
        d.set_folder_here(host, ".config/omarchy", false)
    });
    lab.write("b", ".config/omarchy/hooks/mine.sh", "b hook\n");
    lab.sync_ok("b");
    assert!(lab.data("b").missing_plugins("b").is_empty());
    lab.write("a", ".config/omarchy/hooks/shared.sh", "a hook\n");
    lab.write("a", ".config/hypr/bindings.lua", "a keys 2\n");
    lab.write("a", ".local/state/omarchy/current/theme.name", "old\n");
    lab.settle();
    assert_eq!(lab.read("b", ".config/hypr/bindings.lua"), "a keys 2\n");
    assert!(!lab.exists("b", ".config/omarchy/hooks/shared.sh"));
    assert!(!lab.exists("a", ".config/omarchy/hooks/mine.sh"));
    assert!(!lab.exists("b", ".config/omarchy/themes/old"));
    assert!(!lab.calls("b").contains("theme set old"));
    // Back in: b takes the shared Omarchy settings, its own saved first.
    lab.edit("b", |d, host| {
        d.set_folder_here(host, ".config/omarchy", true)
    });
    lab.sync_ok("b");
    lab.settle();
    assert_eq!(lab.read("b", ".config/omarchy/hooks/shared.sh"), "a hook\n");
}

#[test]
#[ignore]
fn the_bar_indicator_goes_on_by_itself_and_stays_off_once_taken_off() {
    let mut lab = pair();
    let ctx = lab.ctx("a").clone();
    assert!(!bar_indicator(&ctx));
    ensure_bar_indicator(&ctx).unwrap();
    assert!(bar_indicator(&ctx));
    let calls = lab.calls("a");
    assert!(
        calls.contains(&format!("plugin enable {BAR_PLUGIN}")),
        "{calls}"
    );
    // Taken off in Settings: it stays off.
    lab.edit("a", |d, host| {
        d.computers.get_mut(host).unwrap().no_bar_indicator = true
    });
    set_bar_indicator(&ctx, false).unwrap();
    ensure_bar_indicator(&ctx).unwrap();
    assert!(!bar_indicator(&ctx));
    lab.sync_ok("a");
}

#[test]
#[ignore]
fn an_app_name_that_is_a_path_never_becomes_a_folder_to_empty() {
    let mut lab = pair();
    lab.write("a", ".cache/omarchy-sync/keep", "still here\n");
    let script = Script {
        command: "true".into(),
        ..Default::default()
    };
    for bad in ["../..", "../../..", "a b", ""] {
        let e = install_with_script(lab.ctx("a"), bad, &script, &|_| {}).unwrap_err();
        assert!(e.starts_with("not an app"), "{e}");
    }
    assert_eq!(lab.read("a", ".cache/omarchy-sync/keep"), "still here\n");
    assert!(lab.exists("a", ".config/hypr/bindings.lua"));
}

#[test]
#[ignore]
fn the_background_timer_tells_the_person_on_the_second_failure_only() {
    let mut lab = pair();
    lab.join("b");
    let ctx = lab.ctx("b").clone();
    lab.fail_git(Some("fetch"));
    background(&ctx);
    lab.check("b");
    assert!(
        !lab.calls("b").contains("notify-send"),
        "{}",
        lab.calls("b")
    );
    background(&ctx);
    lab.check("b");
    let calls = lab.calls("b");
    assert_eq!(calls.matches("notify-send").count(), 1, "{calls}");
    assert!(calls.contains("Syncing isn't working"), "{calls}");
    background(&ctx);
    assert_eq!(lab.calls("b").matches("notify-send").count(), 1);
    // Back online: a's plugin waits here, which is news once.
    lab.fail_git(None);
    background(&ctx);
    lab.check("b");
    background(&ctx);
    let calls = lab.calls("b");
    assert_eq!(calls.matches("notify-send").count(), 2, "{calls}");
    assert!(calls.contains("Plugins are ready to add"), "{calls}");
}

#[test]
#[ignore]
fn a_forgotten_computer_and_a_renamed_one_both_survive_syncing_at_once() {
    let mut lab = pair();
    lab.join("b");
    lab.add("c");
    lab.join("c");
    // a forgets c while b renames itself: both changes to the computers list stay.
    lab.edit("a", |d, _| {
        d.computers.remove("c");
    });
    lab.edit("b", |d, host| {
        d.computers.get_mut(host).unwrap().name = "Laptop".into()
    });
    lab.sync_ok("a");
    lab.sync_ok("b");
    lab.settle();
    let computers = lab.data("a").computers;
    assert_eq!(computers["b"].name, "Laptop");
    // c comes back as soon as it syncs again, as before.
    assert!(computers.contains_key("c"));
}

#[test]
#[ignore]
fn a_plugin_that_is_a_symlink_stops_being_shared_cleanly() {
    let mut lab = pair();
    lab.write("a", "Code/link/manifest.json", r#"{"name":"Link"}"#);
    let home = lab.ctx("a").home.clone();
    std::os::unix::fs::symlink(
        home.join("Code/link"),
        home.join(".config/omarchy/plugins/link"),
    )
    .unwrap();
    lab.edit("a", |d, host| d.place_plugin(host, "link", true));
    track_plugin(lab.ctx("a"), "link", true).unwrap();
    lab.sync_ok("a");
    let source = lab
        .ctx("a")
        .src
        .join("dot_config/omarchy/plugins/symlink_link");
    assert!(source.exists());
    assert_eq!(
        source_to_target("dot_config/omarchy/plugins/symlink_link"),
        ".config/omarchy/plugins/link"
    );
    // Switched off: its symlink entry leaves the sync folder like a folder would.
    lab.edit("a", |d, host| d.place_plugin(host, "link", false));
    track_plugin(lab.ctx("a"), "link", false).unwrap();
    assert!(!source.exists());
    lab.sync_ok("a");
    assert!(lab.exists("a", ".config/omarchy/plugins/link/manifest.json"));
}
