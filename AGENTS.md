# Notes for coding agents

Read [CONTRIBUTING.md](CONTRIBUTING.md) first; it has the layout, the checks and the writing rules.

- Run `pnpm audit:static` and `pnpm test` after any TypeScript change, and `pnpm audit:rust` after any Rust change. All must pass with no warnings.
- Any change to the engine under `src-tauri/src/sync/` gets a scenario in `src-tauri/src/sync/scenarios.rs` (see "The sync lab" in CONTRIBUTING.md). The lab checks that no settings are lost after every step, so a scenario only has to tell the story.
- Never run the app or the engine's functions against the real home folder while testing. Use a temporary `HOME` and set every `XDG_*` variable too: chezmoi and git find their folders through them.
- Anything the page sends to a Tauri command is untrusted. Validate it in `src-tauri/src/main.rs` (see `known_folder` and the checks in `install_helpers`, `install_apps` and `undo`).
- Don't add inline lint-disable comments; fix the code, or change `.oxlintrc.json` with a reason.
