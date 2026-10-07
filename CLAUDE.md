# Kelvo

Open-source macOS system monitor. A menu-bar tray plus popover, a dashboard window, and
(v2) desktop widget boards, showing CPU, GPU, memory, power and sensors, network, disk and
battery at 1 Hz with local history. Tauri 2 + Rust engine, React UI.

## Development Phase

**Pre-v1.** Breaking changes are encouraged if the result is cleaner. Don't waste time on
deprecation warnings, backwards compatibility, or migration paths. The exceptions are the
one-way doors in `plan/architecture.md` (series data model, host identity, wire format):
get those right, don't iterate on them casually.

## Start here

- `plan/README.md`: what is being built and how the plan is organized
- `plan/PROGRESS.md`: what is done, in progress, and next. Update it when you finish a task
- `plan/design-system.md`: tokens, module accents, type, surfaces, component inventory

**Every UI feature is checked against the design system and the screens already built
before it is called done.** Read `plan/design-system.md` and look at the neighboring screens
before building, screenshot the result (browser dev server + mock transport, plus the dev
gallery at `/?route=/dev/gallery`), and compare. Name every difference you leave in place and why.

## Architecture

```
crates/                    Rust workspace (planned)
  kelvo-schema/           metric catalog, series keys, HostInfo, Capabilities, Snapshot view
  kelvo-proto/            framing, CBOR codec, handshake, message types
  kelvo-collect/          Collector trait; macos/ (sysinfo, IOReport, SMC/HID, IOKit); linux/ stub
  kelvo-store/            SQLite: series interning, tiered rollups, gaps, cursors, pruning
  kelvo-engine/           sampler loop (Ticker, PowerSignals), rollups, detectors, bus
src-tauri/                 app shell: HostRegistry, LocalSource, settings owner, tray, windows, commands
src/
  core/                    plain TS, no React (~ @core/): transport, generated bindings, formatters, chart math
    generated/             tauri-specta bindings. Generated, never hand-edit
  app/                     React (~ ~/): routes/<route>/{_components,_hooks,_lib}, components/{ui,charts}, widgets/, hooks/
plan/                      planning docs (owned by the planning process; read, don't restructure)
```

**Critical rules:**

- Crate dependencies run one way: schema → {proto, collect, store} → engine → app shell. See `.claude/rules/rust.md`.
- `src/core/` has no React imports. `src/app/widgets/**` is render-only: props in, no
  transport, stores, router or `@tauri-apps/*`. Biome (`noRestrictedImports` overrides) and a write hook both enforce this.
- Rust types are the single source of truth for IPC. Regenerate `src/core/generated/`, never edit it.
- The frontend never calls Tauri directly; everything goes through `@core/transport`, which
  has a Tauri Channel implementation and a mock used by the browser dev server, Vitest and Playwright.
- Storage and sync work on series (`metric_id` + labels). Gaps are explicit; never interpolate.
- The idle-CPU budget (under 0.5% across app + WebKit helpers) covers the backgrounded app: menu bar only, no popover or dashboard. Visible UI has no product budget but is regression-gated (D-088). Every 1 Hz render path still matters.

## Rules index

Rules in `.claude/rules/` attach automatically by path. Read one directly when you need it.

| Rule | Covers |
| ---- | ------ |
| `rules/data-boundary.md` | Rust owns what data means (periods, gaps, semantics); the client only does view-dependent shaping |
| `rules/frontend/react.md` | Two-layer structure, aliases, typed IPC, scoped zustand stores, 1 Hz selectors, widget boundary |
| `rules/frontend/styling.md` | Tailwind v4 ladder, tokens, module accents, 1 Hz rendering stability |
| `rules/frontend/motion.md` | Motion tokens, streaming-chart motion, reduced motion, idle-CPU cost |
| `rules/frontend/error-handling.md` | Typed command results, gaps vs errors, channel failures |
| `rules/frontend/testing.md` | Mock transport, Playwright against the dev server, render-count tests |
| `rules/rust.md` | Crate boundaries, thiserror/anyhow, no unwrap, cfg gating, private-API isolation |
| `rules/generated-bindings.md` | tauri-specta output is generated; how to regenerate |
| `rules/testing.md` | Scoped-run ladder, mocking boundaries, timing fakes, flake triage |
| `rules/verification.md` | What "done" means: suites, screenshot comparison, measured perf claims |

Skill: `frontend-design-slop` before adding headers, card grids, badges or stat tiles.

## Prerequisites

- **bun** (frontend package manager and runner)
- **Rust stable** with `rustfmt` and `clippy` (`rustup component add rustfmt clippy`)
- **Xcode Command Line Tools** (macOS SDK for the collectors)
- **pre-commit** (via `uvx pre-commit` or `brew install pre-commit`), then `make hooks`. The pre-push hook runs the macOS checks hosted CI skips (D-096)

## Debugging Budget

If a fix doesn't resolve the issue after 2 attempts without running a new diagnostic step in between, stop. Write "2-attempt limit reached. What I learned: [summary]", list 2-3 alternative root cause hypotheses, and ask which to investigate. Running a new diagnostic step between attempts resets the count, not evidence-based iteration.

## Worktrees

- **Git commands run from the worktree root** with root-relative paths.
- **One plain command per Bash call in isolated worktree sessions.** Claude Code's worktree isolation refuses `cd <worktree> && ...`, loops, and variable assignments wrapped around git or file operations as too complex to verify. The session is already in the worktree: run `git status`, `git add <root-relative path>`, `git commit ...` as separate calls.
- **A fresh worktree needs `bun install`** before hooks, lint or tests work there.
- **Lifecycle is orchestrator-owned.** Subagents in a worktree commit and push their branch only — never `ExitWorktree`, `git merge`, or `git worktree remove`.

## Quick Commands

```bash
bun run dev                # Full app (tauri dev: Vite on :1420 + Rust)
bun run dev:fast           # Browser only, mock transport
bun run typecheck          # tsc
bun run lint               # Biome lint (includes boundary rules)
bun run format             # Biome format + import/class sorting + safe fixes
bun run test               # Vitest (scope with: bun run test -- <path>)
bun run test:e2e           # Playwright against the dev server + mock transport
bun run check              # biome check + typecheck + test
make check                 # Frontend + Rust: what the pre-push hook runs, with bindings-check and e2e-perf
make rust-check            # cargo fmt --check + clippy -D warnings
make bindings              # Regenerate src/core/generated/ from Rust
```
