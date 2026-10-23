# Omarchy Sync

Keeps your [Omarchy](https://omarchy.org) settings, themes, plugins and apps the same on all your computers.

Your settings are kept in a private `omarchy-sync` repository in your own GitHub account. Nothing goes anywhere else. [chezmoi](https://chezmoi.io) does the file work underneath; you never need to touch it.

## What it does

- **Sync Now** saves this computer's changes, brings in your other computers' changes, applies them, and shares the result. Before anything is applied, the Sync pane shows what is coming to this computer.
- **Syncs automatically** every 15 minutes through a systemd user timer, even with the window closed. It notifies you only when a choice is needed or syncing fails twice in a row. You can turn this off per computer.
- **Apps**: choose which apps belong on which computer. New apps wait for your approval before installing (through `yay`, which asks for your password) unless you allow installing without asking. Switching an app off never uninstalls it.
- **History**: every sync is kept. Undo reverses one change on all computers.
- **When the same setting changed on two computers**, you choose which version to keep. The buttons name the other computer and say when it changed.
- **Omarchy, Plugins and App Data**: choose which settings and app folders sync, or keep a separate version of a folder on one computer. Plugins sync with your Omarchy settings.
- Your active theme follows you. `~/.config/hypr/monitors.lua` never syncs, because every screen is different.

Not synced: deleting a file or uninstalling an app on one computer (do that on each), and the git history inside plugin folders (plugin files sync, their `.git` folders don't).

## Install

Omarchy Sync is built from source for now. You need [mise](https://mise.jdx.dev), which Omarchy already includes, and Tauri's system libraries (`webkit2gtk-4.1`, already installed on Omarchy).

```bash
git clone https://github.com/tsubaie/omarchy-sync-app.git
cd omarchy-sync-app
mise install                      # the Node, pnpm and Rust versions in .mise.toml
mise x -- pnpm install
mise x -- pnpm tauri build --no-bundle
install -Dm755 src-tauri/target/release/omarchy-sync ~/.local/bin/omarchy-sync
install -Dm644 omarchy-sync.desktop ~/.local/share/applications/omarchy-sync.desktop
```

Then open **Omarchy Sync** from the app launcher. The first run says exactly what it will install and do before each step: install the tools it needs, sign you in to GitHub, and then either save this computer's settings (your first computer) or bring in your synced ones (every other computer, after backing up the current ones to `~/.local/share/omarchy-sync/backups/`).

## Keyboard and accessibility

Every control is reachable with Tab, and the sidebar works with the arrow keys. Shortcuts: **Ctrl+1** to **Ctrl+7** switch panes, **Ctrl+R** or **F5** syncs, **Ctrl+Q** quits. Controls come from [React Aria](https://react-spectrum.adobe.com/react-aria/), so screen readers can use them, and every pane is checked against WCAG 2.2 AA in the test suite.

Colours come from your active Omarchy theme and are adjusted to meet WCAG contrast. Animations stop when Hyprland or GNOME animations are turned off.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

MIT, see [LICENSE](LICENSE).

## Credits

Built with [Tauri](https://tauri.app), [React](https://react.dev) and [React Aria](https://react-spectrum.adobe.com/react-aria/). Icons from [Lucide](https://lucide.dev) (ISC). The [Inter](https://rsms.me/inter/) typeface (SIL Open Font License) stands in for Apple's SF Pro, which may only be used on Apple platforms.
