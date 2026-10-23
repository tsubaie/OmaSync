#!/bin/sh
# Omarchy Sync quick install, for Omarchy (Arch Linux, x86_64):
#
#   curl -fsSL https://raw.githubusercontent.com/tsubaie/OmaSync/main/install.sh | sh
#
# Downloads the latest release's Arch package, checks it against the release's
# SHA256SUMS, and installs it with pacman (it asks for your password). Nothing is
# compiled and nothing outside the package is touched. Run it again to update.
#
#   OMARCHY_SYNC_VERSION=0.2.0   install that release instead of the latest
set -eu

repo="tsubaie/OmaSync"

say()  { printf '%s\n' "$*"; }
fail() { say "install.sh: $*" >&2; exit 1; }

fetch() {
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then wget -q "$1" -O "$2"
  else fail "curl or wget is required"
  fi
}

# The release JSON is pretty-printed one key per line; python3 parses it
# properly when present, otherwise sed takes the first top-level tag_name.
latest_version() {
  if command -v python3 >/dev/null 2>&1; then
    python3 -I -c 'import json, sys
tag = json.load(open(sys.argv[1]))["tag_name"]
print(tag[1:] if tag.startswith("v") else tag)' "$1" 2>/dev/null && return 0
  fi
  sed -n 's/^[[:space:]]*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' "$1" | head -n 1
}

build_from_source="https://github.com/$repo#build-from-source"
command -v pacman >/dev/null 2>&1 || fail "the package is for Omarchy (Arch Linux). On other systems, build from source: $build_from_source"
[ "$(uname -m)" = x86_64 ] || fail "the package is x86_64 only. Build from source: $build_from_source"

if [ "$(id -u)" -eq 0 ]; then sudo=""
elif command -v sudo >/dev/null 2>&1; then sudo=sudo
else fail "installing a package needs root or sudo"
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

# Which release ---------------------------------------------------------------
version="${OMARCHY_SYNC_VERSION:-}"
if [ -z "$version" ]; then
  fetch "https://api.github.com/repos/$repo/releases/latest" "$tmp/release.json" ||
    fail "could not find the latest release. See https://github.com/$repo/releases"
  version="$(latest_version "$tmp/release.json")"
  [ -n "$version" ] || fail "could not work out the latest release"
fi
version="${version#v}"
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' || fail "expected a version such as 0.2.0, got '$version'"
base="https://github.com/$repo/releases/download/v$version"
file="omarchy-sync-$version-1-x86_64.pkg.tar.zst"

# Download and verify ----------------------------------------------------------
say "Downloading Omarchy Sync $version ($file)"
fetch "$base/$file" "$tmp/$file" || fail "could not download $base/$file (is v$version a published release?)"
fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || fail "could not download $base/SHA256SUMS"
expected="$(grep " $file\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)"
[ -n "$expected" ] || fail "$file is not listed in SHA256SUMS"
actual="$(sha256sum "$tmp/$file" | cut -d' ' -f1)"
[ "$actual" = "$expected" ] || fail "checksum mismatch for $file; nothing was installed"

# Install ----------------------------------------------------------------------
# sudo reads the password from the terminal, not from the piped script.
$sudo pacman -U --needed --noconfirm "$tmp/$file" </dev/tty

say ""
say "Omarchy Sync $version is installed. Open it from the app launcher (Super + Space) or run: omarchy-sync"
