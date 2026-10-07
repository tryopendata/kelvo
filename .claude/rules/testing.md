---
paths:
  - "src/**"
  - "tests/**"
  - "src-tauri/**/*.rs"
  - "crates/**/*.rs"
---

# Testing Rules

Cross-cutting testing doctrine. Stack-specific patterns live in the area rules below.

## Area-Specific Rules

| Area     | Rule File             | What's only there                                         |
| -------- | --------------------- | --------------------------------------------------------- |
| Frontend | `frontend/testing.md` | Mock transport, Playwright, async waiting, render counts  |
| Rust     | `rust.md`             | Collector fixtures, cfg gating, codec skew tests          |

## Test Commands

```bash
bun run test           # Vitest
bun run test:e2e       # Playwright (browser + mock transport)
cargo test --workspace # Rust
make test              # Both
make check             # Everything CI runs: format check, lint, typecheck, tests
```

## Scoped test runs are the default

Escalate only as far as you need to:

1. Single test while red/green cycling: `bun run test -- src/core/format.test.ts -t "bytes"`,
   `cargo test -p kelvo-store rollup::closes_bucket`
2. The test file, once that test passes.
3. The directory or crate, to catch neighbours you broke.
4. The package suite with no args (`bun run test`, `cargo test --workspace`) when the change is done.
5. `make check` ONCE, as the final gate before commit or PR.

Don't scope for: the final gate; flake investigation; anything after a change to a shared
fixture, `vitest.setup.ts`, or a dependency/lockfile; cross-cutting refactors where you can't
name the blast radius.

## Mocking

| Area     | Boundary            | Notes                                                   |
| -------- | ------------------- | ------------------------------------------------------- |
| Frontend | `@core/transport`   | Mock transport; never mock internal modules             |
| Rust     | `Collector`, `Ticker`, `PowerSignals` traits | Fake implementations; real SQLite (in-memory or temp file) for the store |

Never mock internal modules. Use real SQLite for store tests.

## Mocks Must Reproduce Production Timing

For timing-dependent logic (the 1 Hz sampler, rollup bucket closing, sleep/wake gaps,
back-off on battery), the fake must reproduce real timing semantics, not just data. Drive
time explicitly through a fake `Ticker` / fake clock, including skipped ticks and a wake
after a long sleep. A fake that always ticks exactly on time hides gap and bucket-boundary
bugs.

Verify any timing regression test by reverting the fix and confirming it goes red.

## Flaky Test Debugging

| Symptom | Likely Cause | Fix |
|---------|--------------|-----|
| Passes alone, fails in suite | Shared state/singletons (a module-level zustand store) | Scoped stores per test via `renderWithProviders` |
| Random timing failures | Real timers racing the tick | Fake timers, explicit frame pushes |
| Rust test flakes on CI only | Wall clock or real IOKit read | Fake `Ticker`/collector; gate hardware reads behind `#[ignore]` |

## Validation Layers: Prove the Checks Bite

For code whose job is to *catch bad data* (sensor sanity bounds, codec version checks,
gap detection, cursor/`Truncated` handling), a passing suite proves the checks run. It does
not prove they constrain anything.

Mutation-test them: inject realistic failures (a counter that wraps, a negative delta after
wake, a sensor reading 0 °C, an unknown `metric_id`, a pruned cursor) and require each to be
caught. Watch for vacuous passes: an empty frame passes any "all values in range" check.

## Spawning Test-Writing Agents

- Instruct test-writing subagents to create/modify test files ONLY (`*.test.ts(x)`, `tests/**`, `#[cfg(test)]` modules, `crates/*/tests/**`). State this explicitly in the agent prompt.
- Before committing subagent output, run `git diff --stat` and verify every changed file is a test file.
