#!/usr/bin/env bash
# Package the SSH stdio bridge alongside the app, and the static Linux agents
# (make agent-linux) as resources under Contents/Resources/agent/. Keep this overlay out
# of tauri.conf.json so cargo test/check and ordinary development need no sidecar.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

PROFILE=release
TARGET="$(rustc -vV | sed -n 's/^host: //p')"
ARGS=("$@")
for ((i = 0; i < ${#ARGS[@]}; i++)); do
  case "${ARGS[$i]}" in
    --debug) PROFILE=debug ;;
    --target)
      i=$((i + 1))
      TARGET="${ARGS[$i]:?--target needs a Rust target triple}"
      ;;
    --target=*) TARGET="${ARGS[$i]#--target=}" ;;
  esac
done

if [[ "$TARGET" != aarch64-apple-darwin ]]; then
  echo "build-app: remote Mac packages currently require aarch64-apple-darwin (got $TARGET)." >&2
  exit 1
fi

CARGO_ARGS=(build -p kelvo-remote-helper --bin kelvo-remote-helper --target "$TARGET")
if [[ "$PROFILE" == release ]]; then CARGO_ARGS+=(--release); fi
cargo "${CARGO_ARGS[@]}"
mkdir -p "$ROOT/src-tauri/binaries"
cp "$ROOT/target/$TARGET/$PROFILE/kelvo-remote-helper" "$ROOT/src-tauri/binaries/kelvo-remote-helper-$TARGET"
# The agents are Linux resources, not app binaries, so they build for their own targets
# whatever the app's; build.rs compiles their digests into this build. They need the
# pinned zig and cargo-zigbuild. Without them the app is packaged without agents and
# says so when adding a Linux host; release.yml runs `make agent-linux` itself and fails.
CONFIGS=(--config "$ROOT/src-tauri/tauri.remote.conf.json")
if tools="$(make -s -C "$ROOT" agent-linux-tools 2>&1)"; then
  make -C "$ROOT" agent-linux
  CONFIGS+=(--config "$ROOT/src-tauri/tauri.agent.conf.json")
else
  echo "build-app: packaging without the Linux agents. $tools" >&2
fi
bun run tauri build "${CONFIGS[@]}" "$@"
