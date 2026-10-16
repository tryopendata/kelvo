#!/usr/bin/env bash
# Release build of Kelvo.app, ad-hoc signed, installed to ~/Applications and opened on
# the dev machine. Ad-hoc ("-") needs no Apple Developer identity; downloaded copies require
# the user to approve the first launch in Privacy & Security.
#
# Usage: bun run build:prod   (or: bun run build:local)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/target/release/bundle/macos/Kelvo.app"

cd "$ROOT"
APPLE_SIGNING_IDENTITY=- bun run build:app --bundles app

bash "$ROOT/scripts/install-app.sh" "$APP"
