# Omarchy Sync

Keeps Omarchy settings, themes, plugins and apps the same on all your computers.
Your settings are stored in a private `omarchy-sync` repo in your GitHub account; [chezmoi](https://chezmoi.io) does the file work underneath.

## Install

```bash
cargo install --path .
install -Dm644 omarchy-sync.desktop ~/.local/share/applications/omarchy-sync.desktop
```

Then open **Omarchy Sync** from the app launcher. The first run installs `chezmoi` and `github-cli` if needed, signs you in to GitHub, and either saves this computer's settings (first computer) or brings in your synced ones (every other computer, after backing up to `~/.local/share/omarchy-sync/backups/`).

## What it does

- **Sync now**: saves this computer's changes, brings in the other computers' changes, applies them, shares the result, and installs any synced apps that are missing (via `yay`, asking for your password).
- **Same file changed on two computers**: you choose which version to keep.
- **Apps**: pick which apps belong on which computer. Unticking never uninstalls.
- **What's synced**: choose folders, or keep a separate version of a folder on one computer.
- The active theme follows you. `~/.config/hypr/monitors.lua` never syncs, since screens differ.

Not synced: deleting a file or uninstalling an app on one computer (do that on each), and git history inside plugin folders (plugin files sync, their `.git` does not).

## Tests

`cargo test -- --include-ignored` also runs a two-computer sync against a local repo (needs `chezmoi` and `git`).
