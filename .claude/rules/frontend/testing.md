---
paths:
  - src/**/*.test.ts
  - src/**/*.test.tsx
  - tests/**
---

# Frontend Testing

Commands, flaky-test triage and the mutation-test doctrine are in the root `testing.md`.
This file covers what's specific to the frontend suite.

## Commands that only work here

```bash
bun run test                    # Vitest, single run
bun run test:watch              # Vitest watch mode
bun run test -- src/core/format # Scoped by path substring
bun run test:e2e                # Playwright against the Vite dev server + mock transport
bun run test:e2e:ui             # Playwright with UI
```

## Test Location

- **Unit/Integration**: Co-located as `*.test.ts(x)` next to source
- **E2E**: `tests/e2e/*.spec.ts`
- **Shared test helpers**: `tests/` (`renderWithProviders`, mock transport fixtures)

## Mocking with the mock transport

There is no HTTP. Data enters the frontend through `@core/transport`, which has a Tauri
Channel implementation and a mock implementation. Tests use the mock: push frames into it
and assert on what renders. Don't `vi.mock("@tauri-apps/api/...")` in individual tests and
don't mock internal modules; the transport seam is the boundary.

- Live data: drive the mock with explicit frames (`transport.push(frame)`) and advance fake
  timers by the tick, so a test reads as "tick 1, tick 2" rather than racing a real 1 Hz
  interval.
- Commands: the mock answers tauri-specta commands with the same `{ status, data | error }`
  shape the generated bindings return. Fixtures are typed with the generated types, so a
  Rust type change breaks the fixture at compile time.
- Gaps and missing capabilities need fixtures too. A suite that only ever feeds complete
  frames never exercises the "not available" and gap paths.

## E2E (Playwright)

Playwright runs against the Vite dev server in the browser with the mock transport, not
the packaged app. tauri-driver/WebDriver does not support WKWebView on macOS, so the real
webview is covered by Rust integration tests plus scripted manual checks.

E2E is for screenshots of each screen in both themes, contrast checks, and flows that
span routes. Every run starts its own dev server on a free port and never reuses one
already listening (see `playwright.config.ts`).

## Async waiting

Never hold an element reference across an `await`. React can re-render and detach it.
Re-query inside `waitFor`.

**Do not raise `asyncUtilTimeout` to at-or-above `testTimeout`.** It converts a specific
assertion failure into an opaque timeout. If a test is slow, find what it is waiting on.

## Render-count tests for streaming

The 1 Hz selector discipline (`react.md`) is a performance contract, so test it: render two
cards, push a frame that changes only one slice, and assert the other card did not
re-render (a render counter via `Profiler` or a spy component). Revert-verify it by
widening the selector and confirming it goes red.

## Performance gate

`tests/e2e/perf-gate.spec.ts` (Chromium only, part of `bun run test:e2e`) samples CDP
script + layout + style time per second on the popover and the Overview at 1 Hz, and fails on
any long task over 50 ms after warm-up. The thresholds live in `perf-budget.json` at the
repo root and nowhere else. **Raising a threshold needs a decision entry in
`plan/decisions.md`** that names the change that made the screen more expensive (D-060).
Lowering one does not. The gate only catches gross regressions; selector discipline is
still the job of the render-count tests above.

## Integration Test Helper

Use `renderWithProviders` from `tests/test-utils.tsx` for components needing the host store,
QueryClient or router. It wires the mock transport and returns it so the test can push frames.
