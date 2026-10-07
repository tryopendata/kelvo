#!/usr/bin/env bash
# Runtime dependency check (D-058): every library a Kelvo binary links must ship with
# macOS, i.e. live under /System/Library/ or /usr/lib/. Anything else (Homebrew,
# /usr/local, @rpath) would be a library the user has to install, which Kelvo never
# requires. A dependency that is genuinely needed is bundled in the .app; doing that is a
# deliberate change to this allowlist, not a silent link.
#
# Usage: scripts/check-deps.sh [binary...]   (default: target/debug/kelvo)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
if [ "$#" -eq 0 ]; then
  set -- "$ROOT/target/debug/kelvo"
fi

status=0
for bin in "$@"; do
  if [ ! -f "$bin" ]; then
    echo "check-deps: $bin does not exist; build it first (bun tauri build --debug)." >&2
    exit 2
  fi
  # The first line names the binary itself; the rest are its load commands.
  libs="$(otool -L "$bin" | tail -n +2 | awk '{print $1}')"
  if [ -z "$libs" ]; then
    echo "check-deps: otool listed no libraries for $bin; refusing to call that a pass." >&2
    exit 2
  fi
  bad="$(printf '%s\n' "$libs" | grep -Ev '^(/System/Library/|/usr/lib/)' || true)"
  count="$(printf '%s\n' "$libs" | wc -l | tr -d ' ')"
  if [ -n "$bad" ]; then
    echo "check-deps: $bin links libraries that do not ship with macOS:" >&2
    printf '  %s\n' $bad >&2
    status=1
  else
    echo "check-deps: $bin links $count libraries, all under /System/Library/ or /usr/lib/."
  fi
done
exit "$status"
