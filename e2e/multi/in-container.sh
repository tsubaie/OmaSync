#!/bin/bash
# Inside the container: build the real app (with its page built in), then drive it.
set -euo pipefail
export CARGO_TARGET_DIR=/cache/target
mise trust /app/.mise.toml >/dev/null
mise x -- pnpm tauri build --debug --no-bundle
export OMASYNC_BIN=$CARGO_TARGET_DIR/debug/omarchy-sync
dbus-run-session -- xvfb-run -a mise x -- node --test e2e/multi/multi.test.mjs
