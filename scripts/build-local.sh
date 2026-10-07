#!/usr/bin/env bash
# Release build of Kelvo.app, ad-hoc signed, installed to ~/Desktop for final testing on
# the dev machine. Ad-hoc ("-") needs no Apple Developer identity; the app runs here but
# Gatekeeper rejects it on any other Mac.
#
# Usage: scripts/build-local.sh   (or: bun run build:local)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/target/release/bundle/macos/Kelvo.app"
DEST="$HOME/Desktop/Kelvo.app"

cd "$ROOT"
APPLE_SIGNING_IDENTITY=- bun run tauri build --bundles app

codesign --verify --deep --strict "$APP"

# Replacing the bundle under a running copy leaves the old binary running.
if pgrep -xq kelvo; then
  echo "build-local: quitting the running Kelvo."
  pkill -x kelvo
  while pgrep -xq kelvo; do sleep 0.2; done
fi

rm -rf "$DEST"
ditto "$APP" "$DEST"
echo "build-local: installed $DEST"
