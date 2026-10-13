#!/usr/bin/env bash
# Shared install/update step for local builds and the curl installer.
set -euo pipefail

APP="${1:?usage: install-app.sh path/to/Kelvo.app}"
DEST="$HOME/Applications/Kelvo.app"

# Finish copying and verify the replacement before interrupting the running app.
mkdir -p "$HOME/Applications"
STAGING="$(mktemp -d "$HOME/Applications/.kelvo-install.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
ditto "$APP" "$STAGING/Kelvo.app"
codesign --verify --deep --strict "$STAGING/Kelvo.app"

# Only stop this user's Kelvo, and allow time for it to exit before replacing files.
if pgrep -u "$UID" -x kelvo >/dev/null; then
  echo "kelvo: quitting the running Kelvo."
  pkill -u "$UID" -x kelvo || true # It may have exited since pgrep.
  for ((attempt = 0; attempt < 50; attempt++)); do
    if ! pgrep -u "$UID" -x kelvo >/dev/null; then break; fi
    sleep 0.2
  done
  if pgrep -u "$UID" -x kelvo >/dev/null; then
    echo "kelvo: Kelvo did not quit within 10 seconds; leaving the installed app in place." >&2
    exit 1
  fi
fi

rm -rf "$DEST"
mv "$STAGING/Kelvo.app" "$DEST"
echo "kelvo: installed $DEST."
open "$DEST"
