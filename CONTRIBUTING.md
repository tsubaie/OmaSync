# Contributing

Thanks for helping. Omarchy Sync is for people who don't want to think about git, so every change is judged by one question: is it obvious and safe for someone who has never opened a terminal?

## Setup

```bash
mise install            # Node, pnpm and Rust, pinned in .mise.toml
mise x -- pnpm install
mise x -- pnpm tauri dev  # the app with live reload
```

On Omarchy, `webkit2gtk-4.1` is already installed. Elsewhere, install [Tauri's Linux prerequisites](https://v2.tauri.app/start/prerequisites/#linux).

## How it's built

| Where                                         | What                                                                                                                                |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `src-tauri/src/sync.rs`                       | The sync engine: chezmoi, git, gh and pacman calls, conflicts, history, undo, the background timer. No UI.                          |
| `src-tauri/src/main.rs`                       | Tauri commands the page calls. Everything the page sends is checked here, because these commands install packages and change files. |
| `src/api.ts`                                  | Typed calls into those commands. Keep its types in step with the Rust structs.                                                      |
| `src/state.ts`                                | The window's state and actions.                                                                                                     |
| `src/panes/`                                  | One file per pane, plus setup.                                                                                                      |
| `src/ui.tsx`, `src/theme.ts`, `src/format.ts` | Building blocks, theme colours and text formatting, with no knowledge of app state.                                                 |
| `src/mocks.ts`                                | Stands in for Rust when the page runs in a browser for the end-to-end tests.                                                        |

`.dependency-cruiser.cjs` enforces that layering: only `api.ts` and `state.ts` talk to Tauri, building blocks don't import app state, and panes don't import each other.

## Checks

Run the same gates CI runs before you open a pull request:

| Command             | What it checks                                                                                                                                                       |
| ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm audit:static` | Oxlint, TypeScript, Prettier, Knip (unused code and dependencies), dependency-cruiser                                                                                |
| `pnpm test`         | Unit tests, including WCAG contrast for every bundled Omarchy theme                                                                                                  |
| `pnpm audit:rust`   | `cargo fmt`, Clippy with warnings as errors, and the Rust tests, including a real two-computer sync (needs `chezmoi` and `git`)                                      |
| `pnpm test:e2e`     | The page in WebKit with Rust mocked, plus axe accessibility checks on every pane                                                                                     |
| `pnpm audit:all`    | All of the above                                                                                                                                                     |
| `pnpm test:multi`   | The real app (built, driven through WebDriver) on several computers in a Docker container, syncing through a local repository. Needs Docker; not part of `audit:all` |

Playwright's WebKit build only runs on Ubuntu-like systems. On Arch, run the end-to-end tests in Chromium with `E2E_BROWSER=chromium pnpm test:e2e`; CI runs them in WebKit.

Pull requests run these on GitHub Actions; that's the normal place to run the slower ones.

The Rust tests never touch your real settings: the two-computer test points `HOME` and every `XDG_*` folder at a temporary directory.

## Releasing

`install.sh` installs the Arch package attached to the latest GitHub Release. To publish `X.Y.Z`: set the version in `package.json`, `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json` (and refresh `Cargo.lock` with a build), then push the tag `vX.Y.Z`. `.github/workflows/release.yml` builds the package from `packaging/arch/PKGBUILD` in an Arch container, writes `SHA256SUMS`, and publishes the release. The workflow refuses a tag whose number differs from the version.

## Decisions to keep

- **Undo is shared.** History's Undo reverses a sync on every computer. Anything that should only affect this computer (Put Back My Settings, the first-sync backup) uses a local copy instead.
- **Omarchy's own settings follow the newest major version.** An older computer pauses them rather than receiving settings it may not understand; it can try them with a saved copy to go back to.
- **No AUR fallback** for apps that come from their maker's repository, and nothing that changes what the system trusts (a repository, install steps) runs without a clear question first.
- **A file that no longer reads after combining is a clash**, never applied.
- **Check with tools that exist** (serde, toml, serde_yaml, `luac -p`) rather than hand-written parsers.

## Writing for the interface

- Name things by what people recognise, never by how they're built: "your private sync space", not "repo"; "Undo", not "revert".
- Every action says exactly what it will do before it does it, and anything that changes the system or other computers can be undone or asks first.
- Buttons and titles use Apple's title-style capitalisation ("Sync Now"); sentences use sentence case.
