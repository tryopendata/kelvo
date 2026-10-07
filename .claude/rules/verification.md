---
paths:
  - "src/**/*.ts"
  - "src/**/*.tsx"
  - "src-tauri/**/*.rs"
  - "crates/**/*.rs"
---

# Verification Before Completion

Before claiming any task is done, run the touched side's suite (`bun run test`,
`cargo test --workspace`) plus `bun run typecheck` / `cargo clippy --workspace --all-targets -- -D warnings`.
That is step 4 of the escalation ladder in `.claude/rules/testing.md`. `make check` runs once
as the final gate before commit.

UI work is not done until it has been compared against `plan/design-system.md` and the
screens already built. Take a screenshot of the running view (browser dev server with the
mock transport) and put it next to the neighboring screens it should match, and the dev
gallery (`/?route=/dev/gallery`) when you touched a shared component. Name every difference
you are leaving in place and why.

## A clean exit code is not a pass

A command that did nothing and a command that succeeded both exit 0. Before quoting a
pass rate, assert the work actually happened: the test count is non-zero, the file you
expected to be checked was in the run.

The same trap has a second form: **verifying a proxy instead of the thing**. Regenerating
`src/core/generated/` is not a green typecheck; run the check CI runs, not the one that
seems equivalent. When a command's real work is done by a subprocess, check the
subprocess's output, not the wrapper's status.

A third form is **reporting a cause you never observed**. If you say a collector returned
nothing because of a missing entitlement, show the output that proves it.

Beware shell cwd resets between commands: use absolute paths in verification runs.

## A green commit is not a complete commit

Pre-commit's stash/restore cycle can return staged files to the working tree, producing a
commit that passed every hook while missing part of the change. Check
`git status --porcelain` after any hook-running commit. Recovery is
`git add -A && git commit --amend --no-edit`.

## Prove regression tests

- **Revert-verify every new regression test.** Reintroduce the bug by hand, run the scoped
  test, confirm it fails with the expected message, edit it back. A test that passes both
  with and without the fix is not a test.
- **After moving any symbol, run the whole typecheck** (`bun run typecheck`,
  `cargo check --workspace --all-targets`). Scoped test runs never select the files whose
  imports you just broke.

## Hardware numbers

Accuracy claims (CPU %, power, temperatures) are verified against macmon / powermetrics on
real hardware, within ±5% and ±2 °C. A unit test with fixture data proves the math, not the
reading. Say which one you did.

## Performance claims

The idle CPU, memory and popover-open budgets in `plan/architecture.md` are measured with
`make bench` (`scripts/bench-coalition.sh`), `make bench-vs-stats`, `footprint` and
`powermetrics`, never estimated. If you didn't measure, say so. A CPU change is measured
against the previous build in the same sitting (parallel engine runs, or alternating
bench bundles via `APP=... NO_BUILD=1`), not against a number from another day: absolute
numbers on the dev machine swing up to 2x with load.

## Performance gates

Every threshold lives in `perf-budget.json` at the repo root and nowhere else. **Raising a
threshold needs a decision entry in `plan/decisions.md`** naming the change that made things
more expensive (D-060 frontend, D-062 engine, D-067 CPU baselines). Lowering one does not.
The CPU gates have a fixed `target` (the product budget, a user decision) and a `baseline`
ratchet: a run more than `regressionPct` over the baseline fails if a second run is too.
After a measured improvement, lower the baseline to the new number.

The product budget (0.5%) covers the backgrounded app only: menu bar items, no popover,
dashboard or onboarding window, the `tray` scenario. Visible UI has no product target; it
has regression guards (D-088): `perf-gate.spec.ts` blocks in the pre-push hook (`make e2e-perf`, D-096), and `make bench` prints
the visible scenarios against `coalition.visible`, advisory until promoted to blocking.

| Gate | Run it | Budget section |
| ---- | ------ | -------------- |
| Allocations per tick (engine core, each collector), OS calls per tick by API family (tray-only holds the background's 2 s tick and slower cadences, D-094), store rows and bytes per hour | `cargo test -p kelvo-engine --test perf_gates -- --nocapture` (part of `cargo test` on macOS) | `engine.allocsPerTick`, `engine.callsPerTick`, `engine.store` |
| Samples per Supported collector; on Apple Silicon the IOReport, SMC and HID collectors must be Supported (VMs and other hosts print `[perf] skipped: no hardware`, a CI notice) | same test | none (cadence-derived) |
| Release engine CPU, 120 s tray-only with the 1 s setting (a 2 s background tick, D-094), plus a 30 s interval run for comparison; prints per-collector thread CPU and the engine's share of the whole-app target | `make perf` (blocking locally, advisory in the manual macOS CI job) | `engine.perf` |
| Whole-app CPU and footprint (app + WebKit helpers): tray against the product budget | `make bench` (local only, hands off the machine) | `coalition.target`, `coalition.baseline` |
| Whole-app CPU with UI visible: popover, Overview, Processes (advisory) | same run | `coalition.visible` |
| Performance mode saving: off/on in parallel pairs, tray on AC, tray backed off, Overview | `make bench-perf-mode` (local only, on AC, hands off) | `coalition.performanceMode` |
| Frontend main-thread time, long tasks | `bun run test:e2e` (`tests/e2e/perf-gate.spec.ts`) | `frontend` |

The `--nocapture` output prints per-collector numbers, so use it to find what moved. The
allocation counter sees Rust heap allocations only; CF/IOKit mallocs are invisible to it.
`make perf` measures load-sensitive CPU: run it on a quiet machine, and compare against a
parallel run of the previous build rather than a number from another day.
