---
paths:
  - "crates/**/*.rs"
  - "crates/**/Cargo.toml"
  - "src-tauri/**/*.rs"
  - "src-tauri/Cargo.toml"
  - "Cargo.toml"
---

# Rust Rules

No opendata equivalent; written for Kelvo. Architecture lives in `plan/architecture.md`.

## Crate boundaries

Dependencies run one way. No cycles, no shortcuts.

```
kelvo-schema  <- kelvo-proto
kelvo-schema  <- kelvo-collect
kelvo-schema  <- kelvo-store
{kelvo-collect, kelvo-store} <- kelvo-engine
kelvo-engine  <- src-tauri (app shell), kelvo-agent (v4)
```

- `kelvo-schema` is pure data: metric catalog, series keys (`metric_id` + labels), HostInfo,
  Capabilities, alert-rule data, the typed `Snapshot` view. No I/O, no platform code.
  The one non-data module is `kelvo_schema::lock` (`LockExt`/`RwLockExt`, poison-recovering
  lock access, std only): every crate already depends on schema, so it lives here rather
  than adding a crate edge.
- `kelvo-proto` is framing, codec (CBOR via `ciborium`), handshake and message types. It
  never touches the store or collectors.
- `kelvo-collect` reads the OS. It knows nothing about storage or the UI.
- `kelvo-store` knows SQLite and series layouts. It knows nothing about collectors.
- `kelvo-engine` wires collectors to the store and bus through the `Ticker` and
  `PowerSignals` traits.
- `src-tauri` is a thin shell: commands, windows, tray, settings ownership. Business logic
  that isn't Tauri-specific belongs in a crate, where it is testable without a webview.

Adding a dependency edge not in this list is a design change. Raise it, don't add it.

Zero runtime dependencies (D-058): the app links only `/System/Library/` frameworks and
`/usr/lib/` libraries. A crate with a C library must use its bundled or vendored build
(like `rusqlite`'s `bundled`); never require a Homebrew or system install. Dev-only tools
(macmon for the accuracy script) stay optional. `make check-deps` enforces this on
the built binary (locally, and in the manual macOS CI job).

## Error handling

- Library crates (`kelvo-*`) define typed errors with `thiserror`. Callers match on variants.
- Binaries and the app shell (`src-tauri`, `kelvo-agent`, scripts) use `anyhow` with
  `.context("what we were doing")` at each boundary.
- Never `anyhow` in a library crate's public API.
- Tauri commands return `Result<T, CommandError>` where `CommandError` is a serializable,
  specta-typed enum, so the frontend gets a typed `{ status: "error", error }`.
- No `.unwrap()` / `.expect()` / `panic!` / indexing that can panic in non-test code. Use `?`,
  `.get()`, or a typed error. The only exceptions are invariants proven locally, with
  `.expect("why this cannot fail")` naming the invariant. Clippy enforces
  `unwrap_used` outside tests (see `clippy.toml` and the workspace lints).
- A sampler tick must never take the process down. A failing collector logs once (rate-limited)
  and yields no value for that tick; the engine records a gap.

## Unverified private APIs

IOReport, SMC, HID sensor reads, NetworkStatistics and any other private or undocumented
macOS API are isolated:

- Each lives in its own module behind the `Collector` trait.
- The collector returns `Option` (or an empty sample) when the API is missing, returns an
  unexpected shape, or fails. Missing is a capability, not an error to bubble up.
- `unsafe` FFI is confined to that module, with a `// SAFETY:` comment on every block.
- Channel/key names that were not verified on real hardware are marked as such in a comment
  and covered by a fixture test of the parsing, never by a test that needs the hardware.
- Collectors declare the entitlements they need, so an `appstore` feature can drop them.

## Platform gating

- Platform code is `#[cfg(target_os = "macos")]` / `#[cfg(target_os = "linux")]`, at module
  level, not sprinkled through function bodies.
- `kelvo-schema`, `kelvo-proto`, `kelvo-store`, `kelvo-engine` and `kelvo-collect` must
  `cargo check` and `cargo test` on Linux (a native ubuntu CI job enforces this; `make rust-linux-check` cross-checks locally). Linux collectors are
  stubs until v4, but they compile.
- Never gate on `target_arch` when you mean the OS, or on the OS when you mean Apple Silicon.

## Lints and formatting

- `cargo fmt --all` (config in `rustfmt.toml`). The format-on-write hook runs it on edited files.
- `cargo clippy --workspace --all-targets -- -D warnings`. Warnings fail the pre-commit hook. Fix them; don't
  `#[allow]` without a comment saying why.

## Data model rules

- Storage and sync work on series (`metric_id` + labels), never typed structs. The typed
  `Snapshot` is a view built from series and is never stored or synced.
- `host_id` (a persisted UUID) threads through every store call, command and channel.
- Gaps are explicit rows. Never interpolate across sleep, a missing series, or a truncated
  sync.
- Exactly one process writes a given SQLite file.
- `seq` and millisecond timestamps are `i64` and stay below 2^53 (documented in
  `kelvo-schema`) so they cross to TS as `number`.

## Testing

- Unit tests in `#[cfg(test)] mod tests` next to the code; integration tests in
  `crates/<crate>/tests/`.
- Store tests use real SQLite (in-memory or a temp file).
- Engine tests drive a fake `Ticker` and fake collectors, including skipped ticks and wake
  after sleep.
- Codec changes need the round-trip and version-skew test (old fixture decoded by new code
  and the reverse).
- Tests that need real hardware are `#[ignore]` with a reason, and run by hand.
