#!/usr/bin/env bash
# Package the SSH stdio bridge alongside the app. Keep this overlay out of
# tauri.conf.json so cargo test/check and ordinary development need no sidecar.
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
bun run tauri build --config "$ROOT/src-tauri/tauri.remote.conf.json" "$@"
