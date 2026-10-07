---
paths:
  - "src/core/generated/**"
  - "crates/kelvo-schema/**"
  - "src-tauri/src/**/*.rs"
---

# Generated Bindings Rules

## Auto-generated files: do NOT hand-edit

`src/core/generated/` holds the TypeScript bindings tauri-specta generates from the Rust
commands, events and `kelvo-schema` types. Rust is the single source of truth.

**Do not:**
- Hand-edit anything under `src/core/generated/`
- Hand-write a TS type that mirrors a Rust type "until the bindings catch up"
- Write the generated files with a shell command (`cp`, `>`, `sed -i`, `tee`)

**Do:**
- Change the Rust type or command (derive `specta::Type`, register the command/event with the
  specta builder), regenerate the bindings, and commit the regenerated files with the change
- A PreToolUse hook blocks direct writes to these files, and `pre-bash.sh` blocks shell writes

Regenerate with `make bindings` (exports via the specta builder in a debug build of
`src-tauri`). CI checks freshness by regenerating and diffing against the committed files.

## Fresh bindings are NOT the check

Regenerated bindings that differ from the committed ones mean every consumer may now be
wrong. After regenerating, run `bun run typecheck` and the frontend tests; the mock
transport fixtures are typed with the generated types, so a change that breaks them
surfaces there.

## Wire compatibility is separate

The webview gets JSON through tauri-specta; agents and controllers (v4) talk CBOR through
`kelvo-proto`. Updated bindings say nothing about wire compatibility. A change to a type in
`kelvo-schema` or `kelvo-proto` also needs the codec round-trip and skew test (old fixture
decoded by new code and the reverse) to pass.

## Spurious diffs

If your change does not touch a Rust type, command or event, the bindings should not
change. A diff there means the generator version or its config moved; find out why before
committing it.
