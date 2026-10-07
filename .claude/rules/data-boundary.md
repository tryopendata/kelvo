---
paths:
  - "crates/**/*.rs"
  - "src-tauri/**/*.rs"
  - "src/core/**/*.ts"
  - "src/app/**/*.ts"
  - "src/app/**/*.tsx"
---

# Data Boundary: Rust Owns Meaning, the Client Presents

Rust collects and processes data and decides what every value means. The client fetches
that data and presents it. Client-side processing is allowed only where it depends on the
view. When in doubt, the work goes in Rust.

The reason is consistency more than speed. The popover, the dashboard, the desktop boards,
the v3 WidgetKit feed (Swift, which can't reuse TypeScript) and synced remote hosts all read
the same data. They have to agree on what it means. Rust also holds the facts the client
can only guess at, such as each series' current sampling period.

## Rust owns the facts about the data

Anything every consumer must agree on is decided in Rust and reaches the client through the
catalog, the live stream or a command, typed by the generated bindings:

- What was measured, and when. Gaps vs. measurements, and gaps vs. errors.
- A series' sampling period, and when that period changes (adaptive cadences, back-off on
  battery, Performance mode).
- What a value represents: an instantaneous gauge, or an average over the span since the
  previous sample (energy-counter deltas). It also decides how long a sample stays valid.
- Units, derived metrics, rollups, calibration, detectors and events.

If the client needs one of these facts and Rust doesn't publish it, add it on the Rust side.
Don't infer it in TypeScript from row spacing, and don't copy an engine constant into TS.
Don't hardcode a list of metric keys that "behave differently" in a component.

## The client owns view-dependent shaping

These transforms depend on the chart, the window or the screen, so they belong in
`src/core/`:

- Laying rows on a grid sized to a chart's window, and downsampling to its pixel width.
- Stacking, axis ceilings, colors and layout.
- Formatting and unit display preferences.

Doing these in Rust would couple the engine to UI layout and multiply per-window streams.

Even here, the client applies rules Rust published and doesn't invent them. For example,
filling the slots a span-average sample covers is a display transform. Which series are span
averages, and over what period, is data from Rust.

## Display values never become history

A value the client fills, holds or smooths for display stays display-only. Rust never writes
display fills into the ring buffer, rollups, the store or sync: those see only measurements
(see `LiveFrame.values` vs. `held`). Gaps stay explicit, and nothing is interpolated into
history.

## Keep the mock in step

The browser dev server, Vitest and Playwright run on `src/core/mock-transport.ts`. When Rust
starts publishing a new fact (a period, a semantic flag, a new message), reproduce it in the
mock in the same change, or the mock and the real app drift apart.

## Wire format

Publishing new facts often changes the live message format, which is a one-way door
(`plan/architecture.md`). Write the decision entry before changing it.
