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

| Command             | What it checks                                                                                                                  |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm audit:static` | Oxlint, TypeScript, Prettier, Knip (unused code and dependencies), dependency-cruiser                                           |
| `pnpm test`         | Unit tests, including WCAG contrast for every bundled Omarchy theme                                                             |
| `pnpm audit:rust`   | `cargo fmt`, Clippy with warnings as errors, and the Rust tests, including a real two-computer sync (needs `chezmoi` and `git`) |
| `pnpm test:e2e`     | The page in WebKit with Rust mocked, plus axe accessibility checks on every pane                                                |
| `pnpm audit:all`    | All of the above                                                                                                                |

Playwright's WebKit build only runs on Ubuntu-like systems. On Arch, run the end-to-end tests in Chromium with `E2E_BROWSER=chromium pnpm test:e2e`; CI runs them in WebKit.

The Rust tests never touch your real settings: the two-computer test points `HOME` and every `XDG_*` folder at a temporary directory.

## Writing for the interface

- Name things by what people recognise, never by how they're built: "your private sync space", not "repo"; "Undo", not "revert".
- Every action says exactly what it will do before it does it, and anything that changes the system or other computers can be undone or asks first.
- Buttons and titles use Apple's title-style capitalisation ("Sync Now"); sentences use sentence case.
