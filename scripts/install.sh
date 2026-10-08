#!/usr/bin/env bash
# Build Kelvo from source and install it to /Applications. Installs the build tools it
# needs (Xcode Command Line Tools, Rust, Bun) when they are missing. Rerun it to update.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/tryopendata/kelvo/main/scripts/install.sh | bash
#   scripts/install.sh            (from a checkout: builds that checkout as-is)
#
# Piped from curl, the source lives in $KELVO_SRC (default ~/.kelvo/src) and is
# fast-forwarded to the latest main on each run. The app is ad-hoc signed: a build made on
# this Mac carries no quarantine flag, so Gatekeeper opens it without a prompt.
set -euo pipefail

REPO_URL="https://github.com/tryopendata/kelvo.git"
APP_DEST="/Applications/Kelvo.app"

say() { printf '\033[1mkelvo:\033[0m %s\n' "$*"; }
die() { printf '\033[1;31mkelvo:\033[0m %s\n' "$*" >&2; exit 1; }

check_mac() {
  [ "$(uname -s)" = "Darwin" ] || die "Kelvo runs on macOS only."
  [ "$(uname -m)" = "arm64" ] || die "Kelvo needs an Apple Silicon Mac (M1 or newer)."
  local major
  major="$(sw_vers -productVersion | cut -d. -f1)"
  [ "$major" -ge 26 ] || die "Kelvo needs macOS 26 or newer (this Mac has $(sw_vers -productVersion))."
}

ensure_xcode_clt() {
  xcode-select -p >/dev/null 2>&1 && return
  say "installing the Xcode Command Line Tools. Finish the dialog that opens; this script waits."
  xcode-select --install >/dev/null 2>&1 || true
  until xcode-select -p >/dev/null 2>&1; do sleep 5; done
}

ensure_rust() {
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  command -v cargo >/dev/null 2>&1 && return
  say "installing Rust (rustup)."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
  . "$HOME/.cargo/env"
}

ensure_bun() {
  export PATH="$HOME/.bun/bin:$PATH"
  command -v bun >/dev/null 2>&1 && return
  say "installing Bun."
  curl -fsSL https://bun.sh/install | bash
}

# Sets SRC to the directory to build. A checkout this script lives in is used as-is;
# otherwise the managed clone is created or updated. (A function writing a variable, not
# printing one: bash 3.2 drops `set -e` inside command substitution.)
find_source() {
  local here=""
  if [ -n "${BASH_SOURCE[0]:-}" ] && [ -f "${BASH_SOURCE[0]}" ]; then
    here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  fi
  if [ -n "$here" ] && [ -f "$here/src-tauri/tauri.conf.json" ]; then
    SRC="$here"
    return
  fi

  SRC="${KELVO_SRC:-$HOME/.kelvo/src}"
  if [ -d "$SRC/.git" ]; then
    say "updating the source in $SRC."
    git -C "$SRC" pull --ff-only --quiet </dev/null
  else
    say "downloading the source to $SRC."
    mkdir -p "$(dirname "$SRC")"
    # main only: the assets branch holds the README media, which the build doesn't need.
    git clone --quiet --single-branch --branch main "$REPO_URL" "$SRC" </dev/null
  fi
}

build_and_install() {
  cd "$SRC"
  say "installing JavaScript dependencies."
  bun install --frozen-lockfile </dev/null
  say "building Kelvo. The first build takes about five minutes."
  APPLE_SIGNING_IDENTITY=- bun run tauri build --bundles app </dev/null

  local built="$SRC/target/release/bundle/macos/Kelvo.app"
  codesign --verify --deep --strict "$built"

  # Replacing the bundle under a running copy leaves the old binary running.
  if pgrep -xq kelvo; then
    say "quitting the running Kelvo."
    pkill -x kelvo
    while pgrep -xq kelvo; do sleep 0.2; done
  fi

  rm -rf "$APP_DEST"
  ditto "$built" "$APP_DEST"
  say "installed $APP_DEST."
  open "$APP_DEST"
}

# Everything runs from main so that `curl | bash` has read the whole script before any
# command can consume stdin.
main() {
  check_mac
  ensure_xcode_clt
  ensure_rust
  ensure_bun
  find_source
  build_and_install
}

main "$@"
