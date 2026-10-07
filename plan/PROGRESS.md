# Progress

This file tracks where Kelvo is against the plan. Task-level checkboxes live in each version doc. This file holds the milestone status and a dated log, so an agent starting fresh can see what's done and what's next without reading every doc.

Agents update this file whenever a phase starts or finishes. Each update adds a log entry: what changed, how it was verified, and anything left open. They also tick the matching checkboxes in the version doc.

## Start here (handoff, 2026-10-05, QA and polish)

State: `main` holds v1.0, v1.1 and v1.2. The app runs from source with `bun run tauri dev`; `make check` and `bun run test:e2e` pass. There is no release path yet, by design (D-078): no DMG, updater, tap or tags until the user calls the dev build ready.

Stage: manual QA and polish (`v1-local-monitor.md` section 8, "v1.x QA and polish"). The user runs the dev build day to day and files bugs and UX issues; each session takes a batch, fixes it with a regression test where one fits, and logs it here. After QA comes the release phase (same section).

How to work a QA session:
1. Read the issues the user filed for the session. Reproduce each in the browser dev server (`bun run dev:fast`, mock transport) when it is a UI issue, or in `bun run dev` (the full app) when it needs real collectors.
2. Check UI fixes against design-system.md and the screens already built, and name any difference left in place.
3. Run `make check` and `bun run test:e2e` before merging. The perf-gate long-task checks fail under heavy machine load (several worktrees building at once) on unchanged code; rerun on a quiet machine before suspecting a regression (see "v1.2 code review fixes").

Manual checks waiting for the user (the QA checklist in `v1-local-monitor.md` lists them all):
- ⌘-drag order persistence of per-module status items, and WindowServer cost with every module in its own item (D-080).
- Alerts in a locally built app: the "Alerts are on" notification and Kelvo's own permission prompt (under `tauri dev` notifications post as Terminal; a Terminal permission prompt may already have appeared during development).
- A week of detector annotations against the v1.2 proposed bars (false pills per day, fan ramps with a process).
- Per-process network and GPU columns against Activity Monitor.
- Network attribution: brush a Chrome and a Safari download on the Network page and check the app rows (D-089).

Not built in v1.2, candidates for QA or v2: clicking an alert notification opening the Timeline (needs a `UNUserNotificationCenter` delegate in a signed bundle, D-084), and the optimized-charging pill (no documented source, D-083).

Settled, don't reopen:
- Performance is accepted as it stands (D-078). The gates in `perf-budget.json` are regression guards. Sampling default stays 1 s with a window open and 2 s in the background; the tray redraws at most every 2 s with a window open and every 4 s in the background (D-077, D-094).
- History: 1 s in memory for an hour, 10 s for 24 h, 1 min for 7 days, 15 min to 30 days (D-076); commits every 5 minutes (D-070); data directory owner-only (D-074).
- Remote is `https://github.com/tryopendata/kelvo.git`.

Known follow-ups (small, not scheduled):
- CPU power source note ("P cores", "uncalibrated") is on the popover Power card and the Power stack, not yet on the Overview card or the Timeline lane.
- `query_history` does not merge the live ring, so the newest up to 5 minutes are missing from SQLite-only readers (Overview 24 h maxima). A ring merge there fixes every reader (D-070).
- `HISTORY_COMMIT_MS` in `src/core/history-state.ts` mirrors the Rust constant by hand.
- Rows dropped by the frontend ring as out of order are silent (no log or counter); a v4 producer must keep resume rows ahead of newer frames (D-072).
- `live-selectors.ts` sums network rates across interfaces skipping nulls, so all-null reads 0; the disk card's per-process ranking adds read and write with `?? 0`.
- Gap band label pill overlaps the x-axis tick row on module charts; a band can take up to a minute to appear after wake.
- An unreadable `settings.json` is treated as a first run (onboarding shows).
- `.window-state.json` and the log directory are not tightened to owner-only (D-074).
- cargo-deny has not run against `deny.toml` yet; the first CI run is its test. No license is chosen for Kelvo, so the license check skips our crates.
- The Biome widget-boundary rule has no fixture test proving it fails lint.
- `EngineStatus::history_held_until` (D-070) reaches neither IPC nor the UI, so after a backward clock step history silently stops being written for up to an hour. Surfacing it in `LiveStatus` or `HistoryHealth`, for the Settings history banner, is the fix.
- `StreamArea` rebuilds its path each tick instead of appending (D-063); fixed-window charts at slow intervals; HostInfo marketing name, GPU core count and primary interface; freeze a v1 decoder fixture at the first release tag.
- Trademark search for "Kelvo" is still open.
- Two `process_control` tests fail when the appstore-feature test build runs them (pre-existing, not part of `make check`).
- The perf-gate spec shares the machine with the other e2e workers; running it as its own single-worker step would make it steadier under load.
- The pushed-events buffer (`RecentEvents`) subscribes on the first `useEvents` mount, so a push before any events list has mounted is not buffered; a list that mounts later reads after the commit, so this only matters if that ordering changes.
- Alert reset edge: after a reset, the hot-process hold covers only the first batch; a reported process sitting between 100% and 200% on that batch, then climbing back over 200%, can alert again once the cooldown ends (narrow, from the final v1.2 review).
- The CPU page core heatmap rebuilds every cell element when a column closes every 10 s; reusing unchanged cells would cut dev-mode long-task risk.

## Milestones

| Milestone | Doc | Status | Notes |
|---|---|---|---|
| Planning package | [README](README.md) | Done | All 8 docs written, three devil's-advocate reviews applied, answers to Q1, Q3 and Q4 recorded (D-028, D-029) |
| Agent ops and dev tooling | `CLAUDE.md`, `.claude/` | Done | `make check` passes: Biome, tsc, Vitest, cargo fmt, clippy and test |
| v1.0 Local monitor | [v1](v1-local-monitor.md) | Done (runs from source) | Phases 0 to 6 done; two architecture reviews fixed; release work moved after QA (D-078); performance accepted (D-078) |
| v1.1 Long-range history, heatmap, CSV, tray styles | [v1](v1-local-monitor.md) | Done (runs from source) | `make check` and e2e pass; ⌘-drag persistence and the WindowServer re-measure are left to manual QA (D-080) |
| v1.2 Per-process net/GPU, annotations, first alert | [v1](v1-local-monitor.md) | Done (runs from source) | 1.2-A network (D-082), 1.2-B GPU (D-085), 1.2-C detectors (D-083), 1.2-D alerts (D-084); two review rounds fixed; the optimized-charging pill and notification click routing are not built |
| Network attribution (per-app history, brush, Apps table) | [D-089](decisions.md) | Done (runs from source) | Live curl within 3%, tray cost +0.04 pp; Chrome/Safari downloads left to manual QA |
| Sample holds and metric kinds on the live stream | [D-090](decisions.md) | Done | Rust publishes kinds and holds, charts join by them. The client-side data-semantics audit findings were decided and fixed in [D-092](decisions.md); the real app was checked over tray-only history with it (2026-10-06) |
| Data-boundary follow-ups | [D-092](decisions.md) | Done; real-app check open | Group A done: engine, settings and history facts generated as constants (SAMPLING_PLANS, METRIC_CODES, tiers), process refusal from Rust. Group B done: net/disk totals as catalog metrics (gap when any part is), HostInfo gpu_dvfs_mhz and boot_mounts, LiveStatus primary_iface and power_source, self.cpu Mean, memory composition over mem_total_bytes, overview disk from disk.used, mock held staleness. Group C done: rollups weigh Mean/Rate samples by their span (capped at the previous sample's hold); the engine keeps the last hour of its emitted bucket rows plus the open buckets in `LiveHub`, and `query_history` merges them over the store (from them alone with history unavailable); `HistorySeries.hold_ms` (slowest period under the current settings, or the bucket) drives the timeline's joins, replacing the 1.5x rule and the ring stitching, and Live re-reads history when a bucket closes; `battery_hours` takes local hour boundaries; `seriesStats`, `bucketAverages` and a shared `windowMean` (popover footer, settings overhead) run on the `seriesWindow` grid; the mock has no `utun3`. `complete_to_ms` is not on the wire: the Live window still ends at the open bucket's start. Real app: the Power stack is continuous over tray-only history (2026-10-06). Open: check Timeline Live, the battery bars and history-unavailable in `bun run tauri dev` on real hardware |
| v1.x QA and polish (manual, from source) | [v1](v1-local-monitor.md) | Next | The user runs the dev build day to day; sessions fix batches of filed issues |
| v1.x Release: distribution, updater, install | [v1](v1-local-monitor.md) | Not started | After QA; the old v1.0 phase 6 distribution items (D-078) |
| v2.0 Widget manifest and popover composer | [v2](v2-customization-widgets.md) | Not started | |
| v2.1 Floating desktop widgets | [v2](v2-customization-widgets.md) | Not started | |
| v2.2 WidgetKit dev build | [v2](v2-customization-widgets.md) | Not started | |
| v2.3 Alerts editor, configurable Overview | [v2](v2-customization-widgets.md) | Not started | |
| v3.x Native distribution | [v3](v3-native-distribution.md) | Not started | Needs a paid Apple Developer membership |
| v4.x Remote hosts | [v4](v4-remote-hosts.md) | Not started | |

Status values: Not started, In progress, Blocked (say on what), Done (with the verification that proved it).

## Log

### 2026-10-07 (refactor pass)

A DRY and library pass over Rust and TS with no intended behavior change, except as listed below.

Rust:
- `kelvo-store`'s writer is a `writer/` module. The three M15 roll-downs share one `roll_into` over a `Fold` trait, and interning has one `intern_id`. `clear_host`, `discard_from` and `trim_to_cap` walk one `HISTORY_TABLES` registry. A `sqlite_master` test fails when a host table is neither registered nor excluded.
- `range_stats` and `fill_battery_hours` moved from the shell into the store. `floor_to`, `ceil_to` and `Tier::bucket_end` live in schema.
- `engine.rs`, `nstat.rs`, `ioreport.rs` and `tray/model.rs` are split along their existing seams.
- macOS IOKit and libdispatch FFI is declared once in collect, behind safe wrappers, and the engine and shell read through it. The shell's sysctl reads go through collect via the engine.
- Poisoned locks are recovered by `kelvo_schema::lock`. That is schema's one non-data module, documented in `rules/rust.md`.
- `tempfile` replaces the hand-rolled test temp dirs.
- The engine's `ProcessView`, `ProcessSort` and `UsageKey` cross IPC directly (D-100), and the generated bindings are byte-identical.

TS:
- Shared helpers: `clamp01`, `ratio`, `percentOf`, `core/time-grid`, `core/format/clock` (hand-built tables, not Intl), `formatRpm`, marketing GB, `matchProcessQuery`, `countNoun`, `probeHistory`, `useNextBoundary`, `useRangeQuery`, `useOpenEdge` and `useElementSize`.
- `SortHeader` and `ColumnDef` for the tables, with ProcessTable's columns as data. TanStack Table was evaluated and not adopted: ProcessTable is the only multi-column sort, and its freeze, null-last ordering and growOnly sizing would stay custom while the row model rebuilds about 800 rows a tick.
- `MeterTrack` for the six bar meters.
- Card `ariaLabel`/`asChild` and `LinkCard`; SectionCard options with a compact header; `SettingsPanel`.
- Chart pieces: `WindowTicks`, percent and ceiling axes, `PLOT_INSET`, a shared tooltip shell and canvas theme helpers. The gallery-only HistoryChart was removed.
- Radix `RadioGroup` for the tray-style picker and `AlertDialog` behind one `ConfirmDialog`.
- `useWindowSeries` runs through `useRingBuckets`. `RowRing` was retired for a column scan.
- The mock transport shares its command preamble and emitters.

Visible changes:
- Missing values show "—" everywhere.
- Popover memory "used" follows the GB/GiB setting.
- Fans read "RPM", and counts are grouped in en-US.
- Bar meters have rounded ends, and a null share or charge shows an empty track. The popover battery bar previously drew full.
- Confirm dialogs don't close on an outside click, and pressing their backdrop no longer clears a chart selection.
- The tray-style radios take arrow keys.

Not done:
- Tier 5 of the plan: `fan.mode` codes, "Other apps" remainders, partial-coverage slack, `selfCpuAverage` and timeline merge rules still decided in TS. Each needs a decision first.
- `d3-time` ticks, a `LiveAreaCard`, and deriving `Transport` from the bindings, which would drop its JSDoc and named types.

Verified:
- `make check` (872 Vitest tests; cargo workspace; perf_gates 7/7 on a quiet machine) and `make bindings-check` (no diff).
- `bun run test:e2e`: 225 passed. The 5 failures (`shell-screens` scroll-to-top, and onboarding step 1 to 2 in chromium and webkit) also fail on 49c5e61.
- Screenshots of each touched page at 900 and 1440 px against 49c5e61, in light and dark. Card headers match pixel for pixel; the bar-meter ends are the only intended visual difference.
- Two code reviews, with findings fixed.
- `live_power_state` and the `dump` example on this Mac.

Not checked in the running app. Under parallel builds, `perf_gates` processes allocations per tick goes over budget on unchanged code; rerun alone before suspecting a regression.

### 2026-10-07 (range aggregates)

The CPU, GPU, Memory and Disk pages work like the Network page (D-099): each chart is brushable, a totals strip shows average and peak (or peak used, or bytes) over the chart window or the selection, and a "<noun> by app" table ranks apps over the same range, expands to processes, searches and quits, with "Other apps" and "System and other" rows where figures add up. The engine's energy ring became a usage ring charged pro rata with covered time; `query_usage_by_app` and `query_series_stats` replace `query_energy_by_app` and `query_network_totals`, and Network's totals and Power's energy table use the shared components. GPU per process is sampled once per 10 s bucket outside Performance mode; IOKit ceilings rose to the measured 19.5 (tray-only) and 30.5 (window) calls per tick, and `make perf` measured 0.320% and 0.346% tray-only against 0.368% for main in the same sitting. Verified: engine and shell tests (pro-rata charging, sleep, memory peaks of many helpers, GPU coverage, remainders and clamping, zero-row filter, series stats through now), Vitest route tests per page (window list, brush, Esc, a click on another card clearing), Playwright brushing on each page, screenshots of each page plain and brushed plus the gallery. The 30 s comparison run in `make perf` reports "ticked 2 of 3 times" on main too; `shell-screens` scroll-to-top and the onboarding step 1 to 2 e2e tests fail on main as well. Not checked in the running app.

### 2026-10-07 (Network totals)

The Network page shows total download and upload over the chart window, or the brushed range, at the right of the throughput card's stat strip (D-098). `query_network_totals` sums the stored `net.rx_total`/`net.tx_total` rollups through now, less gaps, so it works in the App Store edition and with Network history off; the mock sums its own history the same way. The selection line under the chart no longer repeats the bytes. Verified: a shell test against a real store (store plus engine rows, an open bucket cut at now, a sleep gap through one and a half buckets, a CPU-only gap ignored, live-only), red with gap subtraction removed; Vitest for the window, the `no-process-network` scenario and a sleep gap ("in 30 min 50 s"); screenshots in the browser dev server at 15m, brushed, 1h with the sleep gap and the App Store scenario, and at 900 px wide, where the totals wrap under the live rates. In the mock the totals and the Apps table's sum differ (separate generators); in the app both read the interface counters. Not checked in the running app.

### 2026-10-07 (live ring from history, run-based seed)

After a restart the live charts started empty even with history on disk, because the LiveHub ring (D-066) only held frames from this session. D-097: at startup the shell reads the last hour of stored 10 s averages for every persisted catalog metric and `LiveHub::warm` puts them in the empty ring under a layout of their own (`WARM_LAYOUT_NO`), each row at its bucket's end with the series' hold, so windows get them through the existing `BackfillEarlier` path. No wire change. Engine tests cover the warm rows, warming doing nothing once a frame exists, unusable bucket widths, uncatalogued metrics, more than an hour of buckets and a first frame inside the last stored bucket; a Vitest drives warm rows then live rows through `visitWindow`. A shell test against a real store found the read asking for 360 points while an off-grid launch spans 361 buckets, so the store merged pairs and the ring warmed at 20 s; `max_points` now has one bucket of headroom. Checked in the dev app: the log shows `live ring warmed from history rows=144`. The 1-hour chart in the running app was not screenshotted.

The seed now looks like Kelvo ran for two to five days at a time, idling overnight, with each run ended by a night with the lid closed (sleep gap) or a few hours with Kelvo quit (`app_not_running`), instead of a sleep gap every night. A 30-day run has 9 gaps.

### 2026-10-07 (seed script)

`bun run seed` (`crates/kelvo-store/examples/seed.rs`) replaces the dev build's history (`com.tryopendata.kelvo.dev`; `bun run seed:prod` or `--prod` for the release build's `com.tryopendata.kelvo`, `--dir` for another, `--days`, `--seed`) with 30 synthetic days, so long ranges, the Timeline, process history and network-by-app have data during development. It copies the host record and the series that have values from the existing database (so cores, sensors and disks match the Mac), moves that file aside with `move_aside`, and writes through the store's `Writer`: S10 for the last day, M1 for the whole range rolled into M15 by `prune`, process snapshots (10 s for 72 h, one a minute before), per-app network buckets, nightly sleep gaps, one `app_not_running` stretch, and fan-ramp, sustained-process and thermal-state events driven by the simulated load. Values are calibrated against a real M3 Max recording. Checked on a copy of the dev data dir: tiers, roll-downs, `processes_at` at both resolutions, `net_by_app` over 24 h and 30 d, the refusal while the store lock is held (nothing moved). Not yet looked at in the running app. Each run leaves another `history-reset-<ms>.sqlite` (about 40 MB) beside the database.

### 2026-10-07 (process ports)

The Processes page shows the TCP ports each process listens on, in every column set after PID, and search matches a port by prefix like a PID ("Name, PID or port"). The Idle wake-ups column is gone from the page (the CPU page's Top processes card keeps it). A new on-demand collector, `process.ports` (`kelvo-collect/src/macos/ports.rs`), fills `ProcessSample.ports` on the process ticks while a visible view sets `ProcessView.ports`, the same path per-process GPU uses; each process is re-read at most every 5 s. The read is `PROC_PIDLISTFDS` plus `PROC_PIDFDSOCKETINFO` per socket, keeping TCP sockets in LISTEN; `socket_fdinfo` offsets come from the macOS 27 SDK and a test listening on a port of its own checks them (red with a wrong offset). A full read of every readable process took about 2 ms on the dev machine (800 processes, 680 sockets). Only the user's own processes, like every row. `WireProcess` (remote wire format) and the store's process snapshot do not carry ports.

Verified: `make check` (Vitest 822; the perf gates failed once on processes allocations while the suites ran in parallel and passed alone and on the rerun), new tests red with the port search removed. Screenshot of the CPU and Network sets and a port search in the browser dev server. Not measured: whole-app CPU with the Processes page open (`make bench`), and the perf gates have no ports scenario.

### 2026-10-07 (brand)

Logo and app icon. The mark is a 4 x 4 slice of the history heatmap in the CPU accent, the newest cell solid. `brand/` holds the pack: `kelvo-icon` and `kelvo-logo` (mark plus an outlined Inter SemiBold wordmark), each with a `-dark` variant whose faint cells lift 20%, and `kelvo-app-icon.svg`, the source for `bun run tauri icon` (ink tile, 1024 canvas on Apple's 824 grid). `src-tauri/icons/` is regenerated from it, keeping only the macOS set; the Windows `.ico` and Store logos are gone from the repo and `bundle.icon`. In the UI, `KelvoMark` sits beside the popover title, above the onboarding title, and at the top of the dashboard sidebar: a 20 px mark with an 18 px wordmark under the traffic lights, divided from the nav by the group hairline and part of the window drag region.

Verified: typecheck, Biome, the popover, onboarding and component Vitest files (71 tests), and screenshots of the popover and onboarding in both themes on the dev server.

### 2026-10-07

Performance plan, phases 0 and 1 (D-094). Phase 0 hardened the bench harness: caffeinate in every bench script, `coalition` and `dump --perf` fail a run the Mac slept through, `KELVO_BENCH_DEFAULTS` and `KELVO_BENCH_MENU_BAR` apply settings in memory, the tray logs its frame counters around the measured window, and the bench scripts refuse a closed lid and a run with no tray frames. Phase 1: with no window showing detail the engine ticks at 2 s, temperatures run every 10 s and processes every 30 s, and the menu bar redraws every 4 s (2 s with a window open). `EngineStatus.backgrounded` carries it; detail interest crossing zero re-chooses the tick before a window resumes, so a window never sees the 2 s status, and an open on a stale frame samples at once. Sampling-plan facts, the Performance mode copy and the mock follow. `perf.sh` checks ticks against the tick `dump` reports; `bench-perf-mode` runs only `overview` by default.

Verified: `cargo test --workspace` and clippy with and without `bench`; engine tests for the background cadences, the open-time sample and the hold floor, a live-stream test that a visible window never gets the 2 s status, and a Playwright check that the popover reopens on 60 s charts after a hidden half hour (each revert-verified). `perf_gates` re-measured at the 2 s tick (smc, hid, libproc and nstat ceilings lowered; battery and disk_capacity allocs per tick raised to 1). Vitest 819, Playwright 219 of 220: "dashboard open, backfilling the hour" fails at 51 to 54 ms against 50, the known Phase 3 item. A `dump --perf` smoke run ticked at 2000 ms. Settings checked against board 13: the board predates Performance mode, so only the shorter changes list differs. Not measured: whole-app CPU with `make bench` (the lid was closed, which pauses the tray).

A ce:code-reviewer pass found 10 issues; fixed: the engine now answers the window before the open-time sample, ring segments are placed from their newest row so that off-grid sample does not shift the hour behind it, a hide and show before the stream task runs no longer lets the 2 s status through, the background tray stays 4 s on battery, Performance mode's window-open effects (menu bar, idle processes) are Rust facts in the copy, the mock sends the status on resume, `perf.sh` guards a missing tick, and tests cover a crossing while paused and one that keeps the tick. Left: the popover-open path still waits for the engine's ack on the main thread (bounded by one tick in flight, at most 200 ms); `engine.perf.baseline` is from the 1 s tick and is re-measured with `make perf` next.

Measured after the commit: `make bench` tray-only 0.672% (from 1.21 to 1.38 earlier the same day; one run, load 4.8 to 7.8, not a parallel A/B), main thread 0.29, engine 0.27. Stats and Activity Monitor measured in the same sitting are in `competitor-benchmarks.md` (Stats 4.1% backgrounded).

Open: a parallel `make bench` against the pre-D-094 build to confirm the drop, then phases 2 to 5 of the plan (tray subview spike, dashboard-open gate, Overview cost, ratchet).

### 2026-10-07 (earlier)

Power & Sensors, Network and Settings batch (D-093). Power & Sensors: the SoC thermal zones table scrolls past 10 rows with a sticky header; a new "Energy by app" table ranks apps by CPU energy over the page's chart window, expands to processes (exited ones marked), searches by app, process or PID, and quits an app's main process or any running child. The Search field, quit flow and process actions moved to `src/app/components` and `src/app/hooks` for sharing; the Network Apps table parts moved to `app-table.tsx`. Network: the header shows the primary interface's local IPv4 and the public IP, click to copy. A brushed range clears with a click on the chart, Esc, or a press on empty space outside the chart and Apps table; the chip's Clear is an ×. Settings: °F is the default for new installs.

Verified: `make check` (Vitest 819), `cargo test --workspace` (the engine allocation gate now feeds the energy ring: tray-only 8.8, window 10.6 allocs/tick), `bun run test:e2e` (217 pass; the perf-gate "dashboard open, backfilling the hour" long-task check fails at 56 to 59 ms against 50 on this machine, and fails the same way on unchanged HEAD in a clean worktree). Real-hardware `live_smoke` resolves Chrome and VS Code helpers to their apps and reports energy on 138 of 801 processes. Checked in the browser against board 16 (the energy table follows its Apps card; there is no board for it) and board 08 (zones). Not measured: whole-app tray CPU with `make bench`.

A ce:code-reviewer pass found 13 issues, all fixed: energy lost across a clock step back, the app Quit picking an exited process with a reused pid, a stale public IP after a Wi-Fi change and on remote hosts, the gate not covering the ring, an O(n²) merge, a cadence-dependent floor, a keyboard-unreachable zone scroll, missing tests, and smaller accessibility gaps.

Open: the mock transport has 10 zones, so the scroll shows only on hardware with more.

### 2026-10-06

Popover cards open their module page in the dashboard (`ModuleCard` `href`/`onOpen`, Cores opens CPU). Hover and keyboard focus lift the glow and border and reveal an `ArrowUpRight` beside the title; press drops the glow to 14%. No focus ring, since WebKit keeps it on a clicked card.

Fixed a cascade bug that hid every card border: `motion.module.css` loaded before `app.css` and opened `@layer components` first, ranking components below base, so preflight's `border: 0 solid` beat `.vt-card` and the hover border never applied. `src/main.tsx` now imports `app.css` first.

### 2026-10-05

The repo is initialized with the create-tauri-app scaffold and the design mock bundle (baseline commit `07d4e0c`).

An extraction script unpacks the 15 mock boards into one folder each, with markup, logic, components and a Playwright-rendered screenshot. All 15 screenshots rendered and were spot-checked (the CPU detail board). The mocks were later retired and removed from the repo (D-095).

The Rust toolchain is installed: rustc 1.99.0.

Research on WidgetKit with a free Apple account came back partly confirmed. It works on your own Mac with a stable self-signed identity, a sandboxed extension, and a temporary-exception file feed. App Groups need a team. Nobody has confirmed the recipe end to end. As a result, the WidgetKit dev build moved from v3 to v2.2, and public distribution stays in v3.1.

The frontend tooling moved from ESLint and Prettier to Biome (D-026). Boundary rules (no React in `src/core/`, render-only `src/app/widgets/`, no direct `@tauri-apps/*`) are `noRestrictedImports` overrides in `biome.json`. Verified with `make check`: Biome, tsc, Vitest, cargo fmt, clippy and test all pass.

The app is renamed from Vitals to Kelvo (D-027), because the Homebrew cask token `vitals` is taken by hmarr/vitals and `kelvo` is free. Docs, product name, bundle identifier `com.riley.kelvo` (now `com.tryopendata.kelvo`, D-095) and crate names use the new name. A trademark search is still open.

Three open questions in v1-local-monitor.md 10.2 are resolved. Q1: the Processes page gets Quit and Force Quit in v1.0, with confirmation, a refuse list and no privilege escalation (D-029). Q3: onboarding step 2 is "Updates and privacy", the update-check switch plus a no-telemetry statement with the history location and size. Q4: Kelvo supports macOS N-1, today 27 and 26, with `minimumSystemVersion` "26.0" and testing on both (D-028).

Phase 1, `kelvo-schema` and `kelvo-proto` are done (checklists ticked in v1-local-monitor.md). Schema: series keys with canonical text form, the 6.1 catalog (59 metrics), host identity, capabilities, tiers, cursors, gaps (nullable module, `module_disabled` only), settings with `validate()`, alert-rule data, and `Snapshot::from_frame`. Proto: u32 BE framing, CBOR codec, `Message`, handshake negotiation, v1 skew fixtures. `#[serde(other)]` does not skip unknown variants with content, so decode peeks the tag (D-038). Serialized naming and the `JsSafeInt` specta marker for i64 are in D-039, which also notes the open forward-compatibility issue for closed enums on the wire. Verified: `make check` passes (schema 41 unit tests plus the specta export test, proto 5 unit tests plus 12 skew tests), and `make rust-linux-check` passes. kelvo-store is next.

Wire-facing and stored enums now have an `Unknown` fallback (D-040, settles D-039's open item). There is one CBOR test per enum, and v2-style skew fixtures live in `crates/kelvo-proto/tests/fixtures/skew/`. `Tier::bucket_ms`/`bucket_start` now return `Option`.

Phase 1, `kelvo-store` is done (checklist ticked). It has:
- a SQLite store with `auto_vacuum=INCREMENTAL`, WAL and `user_version` migrations;
- a single writer thread that commits every 30 s and on flush;
- series and layout interning;
- idempotent tier, gap and event upserts;
- a startup `app_not_running` gap;
- process snapshots with a 72 h roll-down to `proc_top_1m`;
- a range query with a min/max/avg merge to `max_points`;
- `processes_at`;
- the cursor read API with `Truncated` driven by a `pruned` table;
- pruning in 5,000-row batches plus `incremental_vacuum`;
- `size_on_disk` including WAL;
- `clear_host`.

Store details the architecture left open are in D-041. `tests/sync.rs` syncs store A into store B through the cursor API and proto `SyncPage`/`SyncRequest` types. It covers a replayed page, a truncated cursor that becomes a gap and an epoch change that forces a full resync. kelvo-proto is a dev-dependency of kelvo-store for that test only.

The synthetic fill test (`crates/kelvo-store/tests/fill.rs`) writes 31 simulated days through the real writer with a daily prune. It is `#[ignore]` so `cargo test` stays fast. Run it with `make test-fill` (release); CI runs it as a step in the Linux job. Measured on the dev Mac, release build, after pruning and close:

| Series | Size on disk | Notes |
|---|---|---|
| 150 | 142.6 MB (142,622,720 bytes) | WAL was 31.8 MB before close. Rows: tier_1m 43,200, tier_10s 8,640, proc_snap 25,920, proc_top_1m 38,880. 14.6 s |
| 250 | 249.2 MB (249,184,256 bytes) | 29.2 s |

150 series is inside the 150 MB budget, with about 5% headroom. 250 series grows faster than linearly, which fits a ~3,000-byte M1 blob landing once per 4 KiB page. A larger `page_size` is the likely lever for big chips (R12), but it is not measured yet.

Verified:
- `cargo clippy --workspace --all-targets -D warnings` is clean;
- `cargo test --workspace` passes (store: 7 unit, 17 integration, 2 sync; fill ignored);
- `bun run check` passes.

Open: `make check` currently stops at `cargo fmt --all --check` on uncommitted kelvo-collect files from the in-progress phase 2 collector work, not on store code. `make rust-linux-check` fails on this Mac because the bundled SQLite in `libsqlite3-sys` needs `x86_64-linux-gnu-gcc`. Schema, proto and collect cross-check clean, and the native Linux CI job builds SQLite with its own gcc.

Phase 2, `kelvo-engine` is done (checklist ticked) except for the overhead target, which it misses. The engine has:
- one input queue (ticks, sleep/wake, device hints, commands) on a utility-QoS thread, with `Engine::pump` driving the same handler in tests;
- `Ticker` (GCD timer on a utility queue, 10% leeway, first tick on a wall-clock multiple), `ThreadTicker` off macOS, `FakeTicker` with a fake clock;
- `PowerSignals` (`IORegisterForSystemPower` with an ack before sleep; battery through a notify(3) token; Low Power Mode, display sleep and screen lock polled every 2 s) and a fake;
- IOKit first-match/terminate hints for `IOMedia` and `IONetworkInterface`, coalesced into one re-probe per tick, with a capabilities revision bump;
- frames with raw `values` (NaN when not sampled) and a `held` latest-value cache (stale after 2.5 sampling intervals) that `LiveFrame::snapshot` reads. The catalog gained a nominal `cadence` per metric for this;
- a one-hour ring with backfill in evenly spaced segments, S10 and M1 accumulators on wall-clock multiples, sleep/pause/shutdown flushes that the store upsert later supersedes;
- sleep gaps measured on the continuous clock, a stall gap when ticks stop without a sleep event, `paused` gaps, `module_disabled` gaps (Power also gaps Sensors), back-off to 2 s on battery or Low Power Mode, a `display_idle` status flag;
- a tokio broadcast bus per host (layout, frames, capabilities, process batches, status) where a lagging subscriber loses messages and the engine never waits;
- the process interest counter, `Source`/`SourceHandle`/`LocalSource`, and `examples/dump.rs`.

Collectors now declare their modules (GPU, IOReport, SMC, sensors, HID), so disabling a module stops the collectors none of whose series remain. Decisions: D-046 (input queue, Ticker and PowerSignals shape, sleep and stall gaps, accumulator flushes, module switches, capability precedence) and D-047 (raw and held values in frames, catalog cadence).

Verified: `make check` passes. Engine: 15 unit tests (1 live test ignored) and 16 integration tests on fakes covering layout-before-frame, NaN for unsampled series, held staleness, S10/M1 bucket boundaries, skipped ticks, a layout change mid-bucket, sleep spans on the continuous clock across an NTP step, sleep inside a bucket, stall gaps, pause, disabling Power, back-off, a lagging subscriber, process interest, capabilities and shutdown. Three mutations (wall-clock sleep span, held values in rollups, flush that resets) each turned a test red. `dump` on the dev Mac (M3 Max, macOS 27.0.1): 170 series, 101 persisted.

Engine-only overhead, measured 10 minutes at 1 s with the release `dump` example writing to a store, CPU from `ps -o time=` after a 15 s warm-up: **1.39%** (8.34 s CPU in 600 s) on the M3 Max, macOS 27.0.1, on AC. That is 7 times the proposed 0.2%. The machine was not idle (unrelated python processes at about 80% CPU each), which adds noise but does not explain the gap. Attribution, 120 s runs per configuration with `dump --disable`:

| Configuration | CPU |
|---|---|
| All modules | 1.49% |
| Power off (also turns off Sensors: SMC power, SMC temperatures and fans, HID zones) | 0.98% |
| GPU off (GPU collector only; IOReport stays for CPU and Power) | 1.46% |
| CPU, Power and GPU off (no IOReport) | 0.28% |
| Everything off (`self.cpu`, processes, the engine itself) | 0.14% |
| Memory, Network, Disk or Battery off, one at a time | 1.43% to 1.56% (no measurable change) |

About 90% of the time is system time (0.11 s user, 1.09 s system over 90 s with everything on): the cost is kernel work behind IOKit calls, not Rust code. IOReport sampling is about 0.65%, the SMC and HID group about 0.5%; `sample` shows 45 `IOHIDServiceClientCopyEvent` round trips every 2 s and one `IOConnectCallStructMethod` per SMC key. The GPU collector's per-tick CF key creation, the suspected hot spot, costs about 0.03% and was left alone. Getting under 0.2% needs product decisions rather than a code fix: fewer HID services or SMC keys per read, slower thermal cadence, or a different target (macmon and Stats make the same calls). Open for the planning process before phase 3.

While measuring, the Mac idle-slept and then only dark-woke. The engine stopped at `WillSleep` and published nothing through the dark wakes, as intended, but `dump --seconds` checked its deadline only on received messages and hung. It now shuts the engine down from a timer thread. Whether a full user wake delivers `DidWake` is still to be checked with a real lid-close.

Open:
- `scripts/accuracy-vs-macmon.sh` is written but not run (macmon is not installed; it exits 2 saying so). Its macmon field names are unverified.
- The real lid-close acceptance test (one `sleep` gap within 2 s of wall-clock sleep) is not done.
- Pruning is not scheduled by the engine; the app shell owns that in phase 3.
- A network interface that becomes active after probe needs a hint or `reprobe` to appear.
- Phase 3 decides whether `LiveMsg::Frame` carries the held values to the frontend (D-047).
- When HID zones work on a chip without an SMC map, Sensors reads `Available` and the unknown-chip state does not show (D-046).
- `make rust-linux-check` still cannot build the bundled SQLite on this Mac, so the engine (which depends on kelvo-store) is only cross-checked by the native Linux CI job.

Phase 3, app shell core is done (the "App shell" checklist in v1-local-monitor.md is ticked). The tray renderer, the popover panel and the windows are the next phase 3 tasks.

`src-tauri` now has:
- `AppState`: `HostRegistry` with the local host, `LocalSource` started at launch and writing to the store, `History` (live-only `history_unavailable` mode if the store fails to open), `SettingsOwner` and `LiveRegistry`. `state.rs` documents the startup order and the API the tray and window code build on.
- The host UUID in `host-id`, recovered from the store's local host if the file is lost (D-051).
- Accessory activation policy, switched to Regular by Show in Dock.
- A settings owner over tauri-plugin-store (Rust is the only writer), typed patches, validation, and `settings-changed` with a revision; engine changes are queued before the event (D-050).
- 16 commands and 3 events, exported with tauri-specta in unified mode (D-048).
- A live channel registry keyed by window label and host, `window_visible`, display-sleep pause, resume with a backfill of only the missed span, and per-window process interest (D-049, which settles D-047's open item: frames carry both raw and held values).
- `get_window_appearance` plus `window-appearance-changed` (power saver, reduce transparency, theme).
- Launch at login through smappservice-rs, `tracing` logs with daily rotation, and store pruning scheduled from the shell (D-051).

Verified:
- `cargo test --workspace` passes; the shell has 26 unit tests: settings validation, persistence and revision; host id persistence and recovery; channel ordering, dedupe, hide/resume and display sleep; interest counting. The live tests were mutation-checked: removing the dedupe, ignoring the last sent row, or skipping the interest adjustment each fails a test.
- `cargo clippy --workspace --all-targets -- -D warnings` is clean, and `bun run check` passes (Biome, tsc, Vitest 68 tests).
- `make bindings` regenerates `src/core/generated/bindings.ts` with no further diff.
- `cargo run -p kelvo` starts. It logs to `~/Library/Logs/com.riley.kelvo/kelvo.2026-10-05.log`, creates `history.sqlite` and `host-id` under `~/Library/Application Support/com.riley.kelvo/`, and starts the engine (161 series, 92 persisted, 15 collectors). A second run read the same host id from the file.
- `make check` fails at `cargo fmt --check` only on kelvo-collect files with uncommitted engine-tuning work by another agent; the shell is fmt-clean.

Open:
- Graceful shutdown on quit (stop hosts, close the store) is wired to `RunEvent::Exit` but unverified: the unbundled binary was stopped with SIGTERM, and there is no tray Quit yet.
- Window hide, minimize and occlusion have to be reported through `LiveRegistry::window_visible` by the window and tray code. The popover should be reported hidden before its page loads.
- `open_dashboard` creates or focuses a plain window and ignores the route.
- On first run, modules are not yet switched off for capabilities the host lacks.
- `sensor_dump` is partial and `check_for_updates` returns `NotConfigured` (D-051).
- The host watcher wakes on every bus message to keep the latest frame; it has not been measured against the idle budget.
- `process_signal` (phase 5) is not started.

Phase 4, frontend foundation, is done (checklist ticked in v1-local-monitor.md). It has:
- `core/transport.ts` (the `Transport` interface and the Tauri Channel implementation), `core/mock-transport.ts` with a seeded generator that lands on the boards' values and scenarios `sleep-gap`, `unknown-chip`, `no-battery`, `no-fans`, `paused`, `stale`, and `core/app-transport.ts`, which picks one from the window and the query string (D-053).
- `core/query-keys.ts`, `core/live-state.ts` (`reduceLive` for all six `LiveMsg` kinds, a one-hour row ring, `seriesWindow` on the interval grid with null gaps).
- A per-host zustand store with `HostStoreProvider` (keyed `hosts[hostId]`, resubscribe with backoff, stale after three intervals), per-module selectors, the settings mirror (drops non-newer revisions, invalidates dependent query keys), the TanStack Query client, and a memory router keyed by window label.
- 23 render-only widgets in `src/app/widgets/`, each with a JSON round-trip test; the uPlot `HistoryChart`; the app components; a dev-only `/dev/gallery`.
- Popover, dashboard Overview and onboarding routes wired to live data; the other dashboard pages are placeholders for phase 5.
- Light `--color-muted-foreground` moved to `#63636b` after the contrast check failed on `#71717a` (D-052).

Verified:
- `bun run check` passes: Biome, tsc, Vitest 177 tests in 35 files. The render-count test goes red when the Memory selector is widened to include `held`.
- `bun run test:e2e` passes, 20 tests on Chromium and WebKit: gallery, popover, dashboard and onboarding in light and dark with no console errors, and the axe check on muted-foreground text. The axe check goes red with the old light token.
- Screenshots of the gallery, popover and dashboard in both themes were compared with boards 02, 03, 04, 07, 08, 14 and 15.

Open:
- Bindings lack: marketing name, GPU core count, memory type and model year on `HostInfo` (machine header uses placeholders); catalog units and cadence; an `unsubscribe_live` command (the frontend drops its channel and relies on Rust noticing); the primary network interface (the popover shows the first in the layout).
- The light `--color-link` `#0891b2` is 3.5 to 3.7:1 on light surfaces; the check is scoped to muted text and does not cover it.
- Not built yet: ProcessTable hover freeze, CollectingOverlay start time, UnsupportedNotice's hidden-module list, more than two tray style options.

Phase 5, CPU, Power & Sensors, GPU and Memory pages are built (checklist ticked in v1-local-monitor.md). They have:
- Routes under `src/app/routes/dashboard/{cpu,power,gpu,memory}/`, wired into the router.
- Shared hooks over the row ring: `use-ring.ts` (held values, layout, window stats, 10 s bucket averages with only the open bucket recomputed per tick), `use-window-series.ts` (bucketed down to 1,200 points for long windows), `use-process-interest.ts`.
- `core/series-stats.ts`, `core/residency.ts` (top four states by share, an "other" row from 0.5%, idle last), `core/format/frequency.ts`, and a `WindowControl` component.
- The dashboard window backfills an hour (`HostStoreProvider backfillMs`), so 15m and 1h windows and the 10-minute heatmap are full on open; the mock seeds 3,600 rows for it.
- The mock transport pushes a process sample as soon as interest turns on, so `ticks=0` pages have rows.
- `PowerStack` keys its x-axis labels by position: with no samples yet the window is 1 s and two labels read "−1s", which raised a duplicate-key error.

Verified:
- Vitest: new tests for series stats, bucket averages, residency rows, cluster naming, pressure state, stack totals, zone re-sort holding for 10 s, and the empty `PowerStack` (red with the old key). Full run is 250 of 251; the failure is `timeline/_lib/gaps.test.ts`, the Timeline work in progress, not these pages. tsc is clean, and Biome is clean on these files (it reports formatting in the Timeline files).
- `bun run test:e2e` passes, 66 tests including other phase 5 specs. `tests/e2e/hardware-pages.spec.ts` (26 of them) screenshots the four pages in light and dark on Chromium and WebKit with no console errors, and covers Show all, no fans, no battery, unknown chip with the dump sheet, and the CPU page across a sleep gap (the total chart breaks and the heatmap column is hatched).

Board 07 (CPU) differences left in place:
- The User stat has no swatch. The board puts the first ramp swatch on User, but the first ramp line in the chart is Total; System keeps the second swatch.
- E-cluster rings and residency use `cpu-2`, as design-system.md's ramp rule says, rather than the board's lighter cyan.
- The total chart has visibly fewer points than the board at 1m because it plots one sample per second.
- The sticky header of the shared `ProcessTable` paints `bg-card` over the card glow, so a flat band shows under the title. That is the shared component's styling; left for its owner.
- The Energy column has no explanatory tooltip: `ProcessTable` headers don't take one.
- Cluster power reads "—" wherever the series is a gap (D-043; D-048 adds SMC keys on mapped chips); the board shows watts.

Board 08 (Power & Sensors) differences left in place:
- No chart annotations ("ANE 1.4 W · Photos face analysis", "Optimized charging: held at 80%"): annotations are v1.2.
- The mock's battery day is flat around 87% with an overnight sleep gap and no charging, so the bars don't look like the board's; the layout, hatch and legend match.
- The mock power stack is noisier than the board and ANE stays at 0, so the hatch band is not visible.
- Zone 10-minute ranges are narrower than the board's because the mock's temperatures vary less.
- On a real Mac with macOS 27, CPU, ANE and DRAM power are gaps most of the time (D-043), so the stack is mostly broken and the package figure reads "—". It shows the gap rather than drawing GPU alone as the total.

Open (missing from Rust or unverified):
- `HostInfo` has no GPU core count or memory type, so the GPU and Memory subtitles leave them out. The GPU frequency ring's max comes from the highest `gpu.residency` state label, because there is no GPU DVFS table.
- No source for memory pressure thresholds, so the pressure chart has no warn or critical lines; the state word comes from `mem.pressure_level`.
- The `fan.mode` encoding (0 Automatic, 1 Manual) is a guess.
- Not checked in the packaged app yet: only the browser dev server with the mock transport.

Phase 5, Network, Disk, Battery and Processes pages are built (checklist ticked in v1-local-monitor.md, except the Rust `process_signal` command and its Rust tests). They have:
- Network: a throughput card (Download hero, Upload, share of link) over a mirrored up/down bar chart with its own ceiling per side, and an Interfaces table.
- Disk: a read/write card with the same mirrored chart, a Volumes table, and the processes table sorted by a new "Disk total" column.
- Battery: Charge, Health and Power ring cards, and the board 08 "Battery, last 24 hours" section (now a shared `BatteryDayCard`) with Full charge and Temperature in its strip. A machine without a battery goes back to Overview.
- Processes: search by name or PID prefix, CPU/Memory/Energy/Disk column sets, row order frozen while the pointer is over the table, a footer with process and thread counts and the D-045 hidden note.
- Quit and Force Quit (D-029): a row action on hover and a context menu, confirm dialogs (Force Quit says unsaved data is lost), refused processes disabled with the reason in a tooltip, and toasts for each outcome including "Kelvo can't quit processes owned by another user".
- `Transport.processSignal`. The mock implements it (refuse list, `PidReused` on a stale start time, `PermissionDenied` for other users, the row disappears on success). The Tauri transport throws a "not implemented" `CommandFailure` until the Rust command lands; the types in `src/core/process-signal.ts` are hand-written and should be replaced by the generated bindings then.
- Shared pieces others can use: `SectionCard`, `BatteryDayCard`, `LiveMirrorChart`, `useBatteryDetail`/`useBatteryHours`, `useUnits`, `labelValues`, and `ProcessTable`'s `rowAction`, `rowContextMenu` and `freezeOrderOnHover` props. The mock now has 24 quiet background processes, including root-owned ones, launchd and Kelvo.

Verified:
- Vitest: 51 files, 271 tests pass. New tests cover the page helpers, the battery hour buckets, the refuse list and signal outcomes, the mock's `process_signal`, the order freeze (red with the freeze removed), and the quit flow on the Processes page: the command is sent only after confirm (red with the confirm skipped), cancel sends nothing, refused rows are disabled with a tooltip, the EPERM toast, and Force Quit from the context menu. tsc is clean. Biome is clean on these files; it still reports formatting in the Timeline files, which are someone else's work in progress.
- `bun run test:e2e`: 66 pass. `tests/e2e/io-pages.spec.ts` screenshots the four pages in light and dark on Chromium and WebKit with no console errors, runs a quit end to end, and checks the no-battery redirect.

Board differences left in place (compared with boards 04, 07 and 08; none of these pages has its own board):
- On the Battery page, the board 08 strip (Charge, Health, Cycles, Remaining) is replaced by Full charge and Temperature, because the ring cards already show the other four.
- The Health ring has no condition word ("Normal"): there is no source for it.
- The hidden-processes note has no number: there is no hidden count in the schema yet.
- The Network page has no per-process section; per-process network is v1.2.

Open (missing from Rust):
- No interface kind (Wi-Fi, Ethernet, VPN), no primary interface (the first in the layout is used), and no since-boot byte counters.
- No volume names, boot volume or APFS container, so volumes are listed by mount path with `/` first.
- `battery.temp` has not been probed on macOS 27.
- The `process_signal` command and its Rust tests.

Phase 3 tray, popover panel, and dashboard and onboarding windows are built (checklist ticked in v1-local-monitor.md; details in D-056). Phase 3 is done.
- Tray: `src-tauri/src/tray/`. The renderer uses tiny-skia and ab_glyph with bundled JetBrains Mono NL (OFL, `src-tauri/fonts/`) and draws the combined glyph and the values layout at 1x and 2x. A `kelvo-tray` thread follows the bus, quantizes each frame, skips frames equal to the last one drawn, sets image and template flag in one call, and sets the accessibility label. It draws nothing while `display_idle` and redraws on pause. Per-module menu bar modes decide which elements show. Left click toggles the popover. Right click or Control-click opens the menu (Open Dashboard, Settings…, Pause Sampling, Quit Kelvo), and Quit exits 0 through the normal shutdown.
- Popover: `src-tauri/src/popover/`. It is a tauri-nspanel non-activating panel, 360 × 680, created hidden at launch. It sits under the status item and is clamped to the screen's visible frame. It hides on focus loss, Esc and a second tray click. An occlusion observer drives `window_visible`. The Popover material falls back to cleared vibrancy under Reduce Transparency. It reloads on the next hide after a WebContent termination. Open latency is measured from click to a double-rAF paint probe that Rust injects, with p95 over the last 50 opens in the debug log.
- Windows: `src-tauri/src/windows.rs`. The dashboard gets an overlay title bar, traffic lights at (18, 22), a 1024 × 700 minimum, and size and position from tauri-plugin-window-state. Close hides it and saves its state; it is destroyed after 5 minutes hidden. `open_dashboard(route)` creates the window at the route, or shows and focuses it and emits `navigate-requested`. The onboarding window (820 × 566) shows while `onboarding.completed` is false. The default `main` window is gone from `tauri.conf.json`.

Verified on the development Mac (M3 Max, macOS 27, debug build; the machine was busy with other agents throughout):
- Rust: 18 new tests (tray model, renderer geometry, width stability, popover placement, latency p95, dashboard routes). The test that temperature text has no ink above or below the bars goes red without the cap-height fix. `cargo fmt`, clippy `-D warnings` and `cargo test --workspace` pass. tsc is clean and Vitest passes 335 of 335. `make check` stops at Biome on 16 formatting errors, all in the frontend agents' uncommitted files under `src/app/routes/dashboard/` and `src/core/`, none in this change.
- Tray values match the engine's dump within one tick: CPU 17/10/6 vs 17.2/9.6/5.7 on the same ticks, memory pressure 9 vs 9.0, hottest 52° vs 51.85.
- Frame skip over 10 minutes: 608 frames, 413 drawn and 195 skipped, a 32% skip ratio, ranging from 20% to 60% per minute. The machine was never idle, so the "half of frames" target is neither met nor disproven. Re-measure on an idle machine once D-055 (thermal sensors every 5 ticks) lands, since temperature changes are part of what defeats the skip.
- Popover open latency over 50 opens: min 28.7, median 48.3, p95 62.8 and max 64.6 ms (budget 150 ms).
- Channel stop: for Esc, outside click and second tray click, "live channel stopped" is logged in the same millisecond as the hide. The occlusion notification follows about 260 ms later. Dashboard close stopped its channel immediately, and the window was destroyed exactly 5 minutes later.
- WindowServer: a direct on/off A/B could not resolve anything, because WindowServer swung between 30% and 57% from other activity. Measured instead with a separate status item swapping images. At 60 Hz the deltas were +2.3, +1.8 and +24.5% (an outlier). At 120 Hz they were +13.5, +4.7 and +13.3%. That is about 0.04 to 0.09% WindowServer CPU per update per second. Kelvo drew 0.68 updates/s, so the estimate is 0.03 to 0.06%, under the 0.2% budget. This is an estimate, not a measurement of Kelvo itself.
- Compared with board 01 by screenshot: the combined glyph matches (three 3 pt bars, 4 pt gap, 30% track, temperature after the bars), and so does the paused state (tracks and a dash).

Board differences left in place:
- Separate tray elements are 10 pt apart; the board shows 14 px between separate status items, which the system spaces, not Kelvo.
- No 150 ms value tween in the tray (design-system mentions one). The tray draws discrete frames.
- The popover page background is opaque, so the material and the 12 pt corner radius don't show yet. That is the frontend's to change.

Not done:
- The D-033 fullscreen first-show anomaly was not reproduced. The user was working on the Mac, and the test switches the display into a fullscreen Space.
- Reduce Transparency was not toggled live, because that changes a system setting. The fallback path is only covered by code review.

Open for the frontend:
- `initialRoute` ignores the URL path, so a dashboard opened at `/dashboard/settings` starts at its default route. It should take `location.pathname` for the dashboard label and listen for `events.navigateRequested`.
- The hidden popover page calls `subscribe_live` again every few tens of seconds while hidden.
- The onboarding window takes focus at launch while onboarding is incomplete; the tray is there too.

Phase 5, Popover (4.3), Dashboard shell and sidebar (4.4), Overview (4.5) and States (4.17) are built (checklists ticked in v1-local-monitor.md, two States items left partial).
- Popover: cards in plan order from capabilities and settings (`popoverSlots`), Radix overlay scrollbar with the header border on scroll, a stale view at 50%, "not available" cards for unsupported modules, and a 60 s self-CPU average in the footer. The power card is a StackBar with an ANE hatch, and the network card shows per-interface held rates in the configured units.
- Shell: the sidebar is driven by `useModuleStates`. Absent and unknown-chip modules are left out, and disabled modules are dimmed with no value. The footer shows Live, Paused, or "Stale · no new data". The dashboard follows `navigate-requested` (only `/dashboard/...` routes) and opens at the URL path Rust gives the window (`initialRoute` gains a pathname). The stale state dims the page to 50%.
- Overview: machine header from `HostInfo` plus live storage and battery, uptime with last wake (the latest closed sleep gap over 7 days). There are six MetricCards built by pure helpers (`overview/_lib/card-props.ts`), and process interest is on while the page is mounted. Process CPU is shown as a share of the machine (cpu_pct ÷ cores). GPU power and disk bars scale to the 24 h maxima, with floors of 5 W and 500 MB/s.
- States: `core/module-state.ts`, `core/history-state.ts` (gap labels and bands, the collecting threshold of 25% of the range, collecting header text, history-unavailable text), and `core/read-failure.ts` ("Sensor read failed · last value HH:MM" as a `CardNotice` on popover and Overview cards).
- New components: `UnavailableCard`, `SensorDumpDialog` (Copy, Save…, Open GitHub issue), `HistoryUnavailableBanner` and `CollectingNote`.
- Mock and transport (additive): a `history-unavailable` scenario, `openUrl`, `onNavigateRequested`, and the mock's `requestNavigate`.
- Follow-ups from the phase 3 native side (75d4e88):
  - A hidden window no longer retries a failed `subscribe_live` (it waits for `visibilitychange` to visible) and no longer flags stale while hidden. On show, the stale timer re-arms from that moment. There is no Rust visibility event, so this relies on WKWebView's `document.visibilityState` for the hidden panel, which is unverified in the app.
  - The popover page is clear in the app (`html[data-native][data-window=popover]`), so the native material shows through the `.surface-vibrant` tint, which goes opaque under `data-reduce-transparency`. The route root has 12 px corners. The browser dev server keeps the opaque page because it has no material.
  - The onboarding window taking focus at launch is not addressed: it is native window creation, outside the frontend.

Verified:
- Vitest: 64 files, 340 tests pass. New tests cover module states, gap labels and bands, collecting, read failure, a series that left the layout drawn as nulls, popover order, card props (process CPU ÷ cores, passive cooling, power source subtitle, link ring, disk used = total − free), history maxima and last wake, the Overview selectors, sampling status, and the shell: navigate-requested is followed for dashboard routes and ignored otherwise, Battery is left out with no battery, and Power & Sensors is left out on an unknown chip. The hidden-window tests (`stores/host-store.test.tsx`), the navigate guard and the unknown-chip tests were revert-verified red.
- tsc is clean. Biome is clean on these files; it still reports formatting in the Settings, Timeline and `settings-patch` files, which are other agents' work in progress.
- `bun run test:e2e`: 98 pass. `tests/e2e/shell-screens.spec.ts` screenshots the popover (default and scrolled), Overview, unknown chip with the dump sheet, no battery, no fans, paused and stale, in both themes on Chromium and WebKit, into `test-results/screens/`.

Board differences left in place:
- Board 02/03/14 (popover):
  - The title says Kelvo, not Vitals (D-027).
  - The GPU third stat is Render, not Cores: there is no GPU core count.
  - The network subtitle is "en0", not "Wi-Fi · en0": there is no interface kind.
  - The memory legend reads App / Cached files, per the existing MemoryBar widget and plan 4.9.
- Board 04/05 (Overview):
  - The title is "MacBook Pro (M4 Pro)", not "MacBook Pro 14-inch (M4 Pro, 2024)", and there is no OS build in parentheses.
  - The chip spec has no "20-core GPU", memory has no "LPDDR5X", model has no "Nov 2024", and storage says "1.0 TB" with no "SSD".
  - The GPU subtitle is empty, the legend is Render/Tiler (plan 4.5), and the body is a 60 s chart until v1.2.
  - Process names are the raw process names ("com.docker.backend", not "Docker"): there is no app display name.
  - The Network list shows interfaces until v1.2.
  - The Disk legend is Used/Free, not Data/System, and the subtitle has no "APPLE SSD": volumes report container size and there is no disk model.
  - The sidebar Network value is rx+tx (plan 4.4), so 39.9M, not 38.4M.
  - There is no Customize button (v2.3) and no Widgets entry (v2.0).
  - Mock energy impact is 41.2, not the board's 412: the mock scales energy differently.
  - The stale dim covers the page header, so the Stale pill is dimmed as well. The sidebar footer carries the state at full contrast.
- Board 12 (states): the unknown-chip notice sits under the Overview grid rather than in its own panel. The sleep-gap and collecting views live on the Timeline (another agent).
- Board 15 (sidebar): matches, apart from the missing Widgets entry and the version string from package.json.

Open:
- Missing from Rust: marketing name and year, GPU core count, memory type, OS build, interface kind and primary interface, swap allocated, a GPU frequency table (the max comes from `gpu.residency` states), disk model, and the Data/System split. `history_unavailable` has no cause (disk full vs corrupt), so the banner has no Reset action. There is no collector error signal (read failure is inferred from held going null while frames arrive). There is no remembered last dashboard route once the window is destroyed.
- `HistoryUnavailableBanner` is not rendered by any history page yet. The Timeline has its own `gapBands` in `timeline/_lib/gaps.ts` that duplicates `core/history-state.ts`, and the two should be merged.
- The sensor issue URL assumes github.com/tryopendata/kelvo. Save… uses a blob download, which is untested in WKWebView.

Phase 5, Timeline, Settings and Onboarding are built (checklist ticked in v1-local-monitor.md under 4.6, 4.15 and 4.16).
- Timeline (`routes/dashboard/timeline/`): 1h and 24h, back, forward and Live. Six uPlot lanes (CPU, GPU, Memory, Power, Temperature, Network mirrored up/down) with min/max envelopes and a shared axis. History comes from one `query_history` per lane: 1h and 24h both ask for `auto` (24h asked for `m1` until d496112, D-076), and the resolution label comes from the returned tier. While following Live, closed ring buckets after the last history bucket are stitched on (history wins), so the line grows one bucket at a time. The crosshair is synced across lanes, with dots, a tooltip and the top 5 processes from `query_processes_at`; it also works from the keyboard (a slider with arrows, Shift+arrows, Home and End). Gap bands carry reason labels. Sleep and Wake markers sit in a two-row annotation row that merges overflow into "+N". Each lane has a "Show as table" dialog.
- Settings (`routes/dashboard/settings/`): a Modules table with the allowed menu bar modes per module and an On switch; absent modules are disabled with "Not present on this Mac". Sampling has the interval with "Kelvo uses about X% CPU at 1s" (the mean of `self.cpu` over the last 10 minutes of the live ring, hidden when there is no reading), Slow down on battery, Keep history (90 days showed "about 430 MB" then; since d496112 the projection follows D-076 and shows about 80 MB), and History on disk (refetched every 60 s) with a confirmed Clear. Units and General are there, plus the update switch and a Version row with Check now. Every control sends one `update_settings` patch, and a rejected patch shows a toast.
- Onboarding (`routes/onboarding/`): step 1 per board 11 (modules from capabilities with Disk off, Combined, Graph per module and Values cards with a live TrayPreview, Launch at login, chip status, Skip and Continue). Step 2 is "Updates and privacy" (the update switch, the no-telemetry statement, the history path, "about 140 MB for 30 days" from retention, and Done). Skip and Done set `onboarding.completed` and close the window.
- Shared additions: `src/core/settings-patch.ts` (option lists, patch builders, presence, projected size from the fill test at 4.75 MB/day, error text), and `Transport.closeWindow()` (Tauri `getCurrentWindow().close()`; the mock records `close_window`).

Verified:
- `bun run check`: Biome clean, tsc clean, Vitest 63 files and 338 tests pass. New tests cover stitching (only closed tail buckets, history wins, rows read through their own layout), the crosshair lookup `bucketAt`, column alignment with null breaks at holes and gaps, gap dedupe, labels and markers, marker layout without overlap and with +N, axis ticks, the settings and onboarding patch builders, the `self.cpu` average and the chip status. I revert-checked one: letting the tail overwrite the last history bucket turns the stitch test red.
- `bun run test:e2e`: 98 pass. `tests/e2e/timeline-settings-onboarding.spec.ts` screenshots each screen in light and dark on Chromium and WebKit with no console errors. It covers the Timeline at 24h and at 1h with the sleep-gap scenario and the crosshair showing processes, Settings writes (Disk off disables its select; Clear goes to 0 MB), and onboarding Continue to step 2.

Board differences left in place:
- Board 06: no 7d/30d, heatmap or Export CSV (v1.1), and no event pill or amber band (v1.2). 24h ticks fall on local hours divisible by 3. The table button on each lane label is an addition. The tooltip flips left near the right edge. Mock data is flatter than the board's.
- Board 13: modules come from settings, so the mock has Network "Hidden" and Disk on where the board shows "Value + label" and off. The mock settings have the battery slowdown off. The board's extra select options (Line graph, Bar histogram and so on) are not offered: `MenuBarMode` does not have them in v1.0. The update switch and the Version row are below Appearance (4.15, not mocked).
- Board 11: the "Graph per module" card (v1.1-D, D-080) previews CPU and MEM only, as the board does; with no samples yet its sparkline box is empty. The traffic lights are the native overlay title bar, not drawn. Step 2 is not mocked and reuses board 11's frame and list well.

Open:
- The Timeline's `gapLabel` and band logic in `timeline/_lib/gaps.ts` duplicates `core/history-state.ts`; merge them.
- The Network lane's min/max is the sum of per-interface min/max, which bounds the true envelope rather than measuring it.
- History refetches every 5 minutes while Live; between refetches the ring tail carries the line.
- The empty-history overlay (4.17) is not wired into the Timeline, although the States checklist says it is.
- `closeWindow` needs `core:window:allow-close` on the onboarding window, which is in `capabilities/default.json`. It is not tested in the packaged app.
- The projected size scales the fill test linearly; the 10 s tier is fixed at 24 h, so 7 days is slightly under-estimated.

History can no longer fill the disk, and Kelvo has no runtime dependencies (D-057, D-058). The changes:
- A 150 MB byte cap (WAL included) on top of time retention. After pruning, the writer checkpoints with `TRUNCATE` and measures the files. If they are over the cap, it trims the oldest history on every host down to 90% of the cap and moves the `pruned` marks so cursors get `Truncated`. It writes no gap and never trims the last 24 h. `PruneReport` now carries `cap_trim` and `size_bytes`.
- New databases use 16 KiB pages with `wal_autocheckpoint` at 4 MiB.
- A low-disk guard (`LowDiskGuard`, `FreeSpace` trait, `statvfs` via rustix) pauses S10 writes under min(2 GB, 5%) free and resumes above 1.5 times that. While paused, `Auto` history queries read M1 over the hole. The shell checks every 5 minutes and after each prune (`History::spawn_pruner`).
- `History::health()` returns `HistoryHealth { low_disk_paused, trimmed_before_ms, cap_met }` for the UI. It is not in the IPC bindings yet.
- `scripts/check-deps.sh`, `make check-deps` and a CI step after the macOS debug build fail if the app links anything outside `/System/Library/` and `/usr/lib/`. A principle in README section 6 and a line in `rules/rust.md` cover the same rule.

Measured with the fill test (release build, dev Mac). Before: 142.6 MB for 150 series and 249.2 MB for 250 series. The WAL before close was 31.8 MB and 55.0 MB, so the real peak at 150 series was about 174 MB. Page size, size after close for 150 / 250 series:
- 4 KiB: 142.6 / 249.2 MB
- 8 KiB: 139.5 / 245.9 MB
- 16 KiB: 139.7 / 203.4 MB

Steady-state WAL per 30 s commit is 59.7 KB at 4 KiB and 154.2 KB at 16 KiB (150 series). With the cap and 16 KiB pages:
- 150 series: 139.7 MB with no trim.
- 250 series: trimmed to 24,999 M1 rows (about 17.4 days), 139.9 MB after close and at most 150.0 MB right after any daily prune.
- The WAL is 0 bytes after every prune.

Verified:
- New `tests/disk_limits.rs` (5 tests) covers the cap trim with cursor and `Truncated` checks, no gap written, low water, the 24 h floor, the file shrinking right after prune, low-disk pause, keep and resume with hysteresis, and the hole closing after a run that ended paused.
- Each of these tests went red under its mutation: no checkpoint, no `pruned` mark on trim, S10 not dropped, low water equal to the cap, and no reopen fixup.
- The fill test now asserts the cap at 250 series.
- `cargo test --workspace`, clippy `-D warnings` and `bun run check` (340 tests) pass. `make check-deps` passes on a fresh debug build (23 libraries), and fails on a Homebrew binary (`brotli`, `@rpath` libraries).
- `make check` stops at `cargo fmt --check` on `src-tauri/src/process_signal/mod.rs`, another agent's uncommitted file.

Open:
- `HistoryHealth` needs a command or a field in `commands.rs`/`ipc.rs` plus bindings before Settings and the history banner can show "History paused: disk almost full" or "History trimmed to stay under 150 MB".
- The Settings projection for 90 days ("about 430 MB") is now bounded by the cap. Superseded: since d496112 the projection follows D-076, and 90 days at 150 series is about 83 MB, under the cap.
- `make rust-linux-check` was not run; Linux CI covers rustix's `statvfs` there.

Phase 5 `process_signal` (D-029) is in, so Quit and Force Quit work in the app, not only against the mock. The guard is `src-tauri/src/process_signal/`, behind a `ProcessOs` trait so tests use a fake. It refuses PID ≤ 1 and Kelvo's own PID, re-reads the start time and returns `PidReused` on a mismatch, then refuses `kernel_task`, `launchd`, `WindowServer`, `loginwindow` and anything Kelvo is responsible for (its WebKit helpers). Regular apps get `NSRunningApplication.terminate`/`forceTerminate`; everything else gets `SIGTERM`/`SIGKILL`. `EPERM` maps to `PermissionDenied` and `ESRCH` to `NotFound`. A process libproc cannot read (another user's) is classified with `kill(pid, 0)` and never signalled. The command is local-only: a remote host returns `RemoteHost`. The macOS reads repeat `kelvo-collect`'s libproc start-time formula, because the shell does not depend on that crate. The generated `SignalKind` and `ProcessSignalError` replace the hand-written TS types, `tauriTransport.processSignal` calls the command, and the toasts cover every variant. The UI refuse list gained `loginwindow`.

Verified:
- `cargo test -p kelvo`: 55 pass, including 10 guard tests on the fake (each refused target sends nothing, `PidReused`, `NotFound`, `EPERM` mapping on read and on send, JSON shapes).
- The 2 `#[ignore]`d live tests pass by hand: `sleep 600` is quit and force-quit (and a stale start time sends nothing first), and root's `syslogd` returns `PermissionDenied`.
- clippy `-D warnings` on `kelvo` is clean. tsc is clean, Biome is clean on the touched files, and Vitest passes on the processes, mock-transport and process-signal suites (34 tests).

Open:
- No mutation run on the guard: the edit to drop the start-time check was denied by the permission system.
- Not clicked through in the running app.

Engine overhead and CPU power follow-up to the phase 2 numbers above (D-054, D-055).

CPU power from SMC (D-054, not adopted). A read-only scan of the M3 Max SMC (2,877 keys, no writes) found 1 Hz P-cluster power keys: `PC02` + `PC03` for P0 and `PC42` + `PC43` for P1. Over whole PMP refresh windows (918 s light load, 352 s with two steady threads) they read 0.75 to 0.79 of the PMP cluster power that powermetrics and macmon report, outside the ±5% bar. No key tracks the E cluster. The SMC path is not shipped; the code is kept as a patch outside the repo. `power.cpu` stays a gap between PMP refreshes. D-054 lists three options for the user: a fixed per-chip scale, live calibration against each PMP window, or a separate 5-minute `power.cpu_avg` series.

Overhead. Method: the release `dump` example writing to a store, CPU from `ps -o time=` after a 15 s warm-up, on AC. Sequential 120 s runs of the same binary ranged from 0.81% to 1.48% with the machine's state (load average 2.3 to 5.8; other agents were building), so variants were compared in parallel 300 s runs, where background load hits both equally.

| Lever | Before | After | Result |
|---|---|---|---|
| Cache SMC key info | | | Already in place (`Smc.infos`); one cached key read costs 0.2 ms vs 1.4 ms uncached at 1 Hz |
| Read only mapped SMC keys | | | Already in place (29 keys on the M3 Max) |
| HID client opened once, services refreshed on probe/wake | | | Already in place |
| Temperatures every 5 ticks instead of 2 (D-055) | 0.957%, 0.757%, 0.990% | 0.803%, 0.653%, 0.830% | −0.15 points, kept |
| IOReport: only groups in use, no per-sample CF re-creation | | | Already in place (3 cluster channels, GPUPH, 7 energy channels; channel key cached) |
| IOReport: drop the PMP energy channels | 5.1 ms per sample | 4.7 ms | about −0.04 points, inside noise; rejected, it loses ANE/DRAM/package when PMP refreshes |
| SMC CPU power keys every tick (D-054 trial) | 0.957%, 0.757% | 0.970%, 0.790% | +0.02 points; moot, not shipped |

Per-call costs at the 1 Hz call rate (a tight loop under-reports them 3 to 10 times): IOReport as subscribed 5.1 ms (0.51%), of which the CPU cluster channels are 3.9 ms; SMC 29 sensor keys 3.0 ms; HID 46 services 3.0 ms. About 90% of the engine's time is system time.

Verified: `cargo test` for kelvo-schema, kelvo-collect and kelvo-engine passes (collect 41 + 4, engine 15 + 16, schema 43 + 12 + 1), and clippy `-D warnings` is clean on them. `dump --json` showed temperatures updating every 5 s and held in between.

Open:
- The engine is still about 0.8% on a busy machine, above the 0.2% soft target. The IOReport cluster channels alone are about 0.4% at 1 Hz, so 0.2% needs a product decision, for example sampling cluster frequency and residency every tick only while a window is open.
- The user chooses a D-054 option for CPU power.
- powermetrics was not available (it needs sudo), so the PMP counters were the accuracy reference.

History size limit, history health in the UI, sampling interval UI, and the frontend perf gate (D-059, D-060).

- **Size limit.** `history.size_limit_mb` is 150 (default), 300, 500 or 1000 MB and maps to `Retention::max_bytes`. Changing retention or the limit prunes right away. Settings shows a projected size for each retention option and "Limited to about N days by the X MB limit". Onboarding step 2 quotes the same model (`core/history-projection.ts`). It was fitted to the D-057 fill test then, where 150 series at 90 days was limited to about 30 days by 150 MB, and 250 series to 17. Since d496112 it is fitted to the D-057 and D-076 fill pairs, and 90 days fits under 150 MB (about 83 MB at 150 series, 116 MB at 250).
- **Health.** The `history_health` command and the `history-health-changed` event carry `HistoryHealth`. The low-disk pause and an unmet cap show as warnings. A trim shows as one muted info line, and on the Timeline only when the range reaches before it. `HistoryUnavailableBanner` now renders on the Timeline, the battery card and Settings.
- **Interval UI.** All 7 intervals are in Settings, and the battery row names the backed-off interval. The popover cards, the footer's self CPU, the Overview GPU card and the popover's first backfill scale with the interval (30 s shows 30 minutes). Module window controls disable windows with fewer than 10 samples. The mock transport takes `?interval=`.
- **Perf gate.** `tests/e2e/perf-gate.spec.ts` (Chromium, CDP) uses thresholds from `perf-budget.json`: popover 40 ms/s, Overview 50 ms/s, and no long task over 50 ms. Measured script + layout + style per second, across runs alone and in the full suite: popover 12.5 to 20.0 ms/s, Overview 16.2 to 25.5 ms/s, with no long tasks. A gate set to 10 ms/s went red. A mutation that re-renders the whole popover every tick measured only 21 to 23 ms/s, so the gate catches gross regressions only.

Verified:
- `make check` passes: fmt, clippy, cargo test with kelvo 63 and schema 43, Biome, tsc, and Vitest with 67 files and 368 tests.
- `bun run test:e2e`: 110 passed, and 2 skipped (the perf tests on WebKit).
- New Rust tests cover the size-limit validation, the pruner wake on a limit change, and 6 `next_health` cases.
- New Vitest tests cover the projection, the live windows, the notices, the mock transport, and Settings.
- New e2e tests in `interval-history.spec.ts`.

Open:
- The tray already redraws on each bus frame, so it follows the interval with no change.
- These keep fixed windows and their "Last 60 s" labels: the 10-minute charts (power stack, core heatmap, swap, GPU power, battery power), GPU frequency, and CPU residency.
- Health is in memory, so a restart forgets the trim note.
- Not compared against board 13: the size-limit row and the notices are not in the mock. Screenshots are in `test-results/screens/settings-history-limit-*.png`.

Sampling intervals, tray-only cadence, CPU power calibration and engine perf gates (D-061, D-054, D-062).

- **Intervals.** 0.5, 1, 2, 5, 10, 30 and 60 s. Collector cadences are wall-clock minimum periods, so disk capacity stays 60 s and temperatures 5 s at any interval. Held values go stale after 2.5 times the largest of the catalog period, the collector's current period and the interval. S10 buckets with no sample are not written and are not gaps. Battery back-off doubles the interval, capped at 60 s. FakeTicker tests run at 0.5, 30 and 60 s (M1 gets 5 rows over 5 minutes at 30 s, and one per minute at 60 s).
- **Tray-only mode.** The engine counts detail interest; the shell adds one per visible window streaming the host. Without it, IOReport (cluster frequency and residency, GPU states, GPU/ANE/DRAM power) samples every 10 s. Those M1 averages stay exact because they are counter deltas; only the min/max envelope narrows. The tray metric set is listed in D-061.
- **Bug found.** A collector with no series keys (processes) was never active, so process rows were never sampled in the real app. It is now sampled every 10 s, or every tick with process interest.
- **CPU power (D-054 accepted, option b).** On the M3 Max `power.cpu` and `cpu.cluster.power{P0,P1}` come from the SMC every tick, scaled by a live calibration against the PMP energy over refresh windows (clamped 0.5 to 2.0, smoothed). The E cluster is left out. Before the first calibration the values are raw and `power.cpu_source` is 1; after, it is 2.
- **Perf gates (D-062).** Allocations per tick, OS calls per tick and store volume run in `cargo test`; `make perf` runs release engine CPU, advisory in CI. Thresholds are in `perf-budget.json`. The gates found per-row string copies in process sampling. Allocations per tick went from 177 to 7.7 tray-only and from 1,563 to 9.5 with a window open.

Measured on the M3 Max, macOS 27, release engine, 120 s at 1 s. Four runs ran in parallel, twice, with load average 2 to 4:

| Mode | Before | After |
|---|---|---|
| Tray-only | 0.96%, 0.99% | 0.61%, 0.63% |
| Window, detail only | 0.99% (IOReport always every tick) | 1.07% |
| Window, detail + processes | 0.95% (processes never sampled) | 2.13%, 2.16% |
| Tray-only at 30 s (`make perf`) | | 0.12% |

Tray-only drops by about a third. With a window open, the processes table at 1 Hz costs about 1.1 points: about 2,600 libproc calls a second, nearly all system time. That is the next thing to cut.

Verified:
- `make check` passes: Rust 278 tests, 0 failed, including `perf_gates`; Vitest 368 tests in 67 files.
- Mutations went red as expected:
  - tray-only IOReport at 1 s fails the call ceiling (1.00 > 0.2);
  - restoring per-row name copies fails the allocation ceiling (1,522 > 1);
  - the cadence mutations went red earlier.
- `live_smc_power_and_fans` on hardware: P0 + P1 = `power.cpu`, and `power.cpu_source` reads 1 before calibration.

Open:
- UI: label `power.cpu` from `power.cpu_source`. On the M3 Max, the Cluster frequency card's E-CLUSTER POWER no longer has a value.
- The hardware calibration check (`live_cpu_power_calibration_matches_pmp`, ±5% out of sample) was stopped at 45 minutes. The PMP counters moved once, at 1,148 s, and not again, so the check never ran: calibrated accuracy on hardware is unverified. Users may see uncalibrated (about 25% low, labelled) CPU power for well over half an hour.
- The allocation counter cannot see CF/IOKit mallocs.

#### Frontend architecture-review fixes (#8, #17, #18, #23, #24)

- **#8 Live chart path (D-063).** The live state keeps typed per-series columns (`SeriesColumns`, Float32Array per key, NaN for missing) written on append; `seriesWindow`, `seriesStats` and `bucketAverages` read them. Bucketed 15m/1h charts recompute only the open bucket per tick. The per-core heatmap re-renders one cell per row per tick. `StreamArea` still rebuilds its path each tick (about 0.8 ms/s on the CPU 1 h page), so the append-a-segment scroll in architecture.md is not done; D-063 says why. The perf gate adds the CPU page on its 1 h window (budget 35 ms/s), and the mock now runs 800 processes.
- **#17 Gap labels.** One implementation in `@core/history-state`: `gapLabel(gap, style)`, `dedupeGaps`, `gapBands(gaps, from, to, { module, style })`; the Timeline maps its span to a style. One wording per reason ("History pruned", "Host offline", generic "No samples"), plus "Clock changed" and "History write failed" for the new `clock_changed` and `write_failed` reasons before the bindings carry them.
- **#18** The Overview uses the shared `useProcessInterest`, which logs a failed `set_process_interest`.
- **#23 Missing is not zero.** `InlineBar`, `StackBar` and `RingGauge` take `null` for a missing measurement and draw the empty track; `InlineBar`'s old "no bar" case is now `"none"` (Passive cooling, Swap). The popover's `share()` returns null; popover power no longer folds a missing rail into "Rest of system"; memory composition no longer inflates shares when a part is missing.
- **#24** Settings changes invalidate per section (`settingsInvalidation`): history and size for retention/limit and module switches, the size projection for sampling, the update check for `check_updates`, nothing for appearance, units or onboarding.

Perf gate, ms/s of main-thread time (M3 Max, dev server, Chromium; "before" is the previous code with the new CPU case and 800-process mock, isolated runs; "after" is isolated and full-suite runs):

| Screen | Before | After | Budget |
|---|---|---|---|
| Popover | 19.2 to 23.0 | 13.9 to 21.1 | 40 |
| Overview | 19.1 to 20.5 | 14.8 to 19.0 | 50 |
| CPU page, 1 h window | 37.2 to 39.3 | 18.2 to 23.2 | 35 |

The process count alone barely moved the numbers (32 vs 800 processes: within run-to-run noise). One full-suite run before the heatmap keying change had a 65 ms long task on the CPU page; three full-suite runs after it had none.

Verified:
- `bun run check` passes: Biome, tsc, Vitest 394 tests in 71 files.
- `bun run test:e2e` passes: 111 passed, three full runs.
- Revert-checked: `share()` returning 0 fails the popover missing-value test; dropping the open-bucket memo key fails the 1 h equivalence test; the Overview interest-error test could not pass with the old local hook (it logged nothing).

Open:
- `StreamArea` append-and-translate (D-063 revisit).
- Gap bands on module page history charts are still unchecked (v1 checklist, States); `gapBands({ module })` is ready for them.
- No mock-board comparison was needed for the normal views (nothing changes when every value is present); the missing-value states have no board.

#### Store, engine and shell architecture-review fixes (#2, #3, #4, #9, #13, #16, #19, #20, #21, #29)

Decision: D-064 (amends D-041). architecture.md infra 2, 3, 4 and 8, the hosts DDL, the Engine section and Startup are updated.

- **#2** The shell registers the host itself (`History::register_host`). If that fails, history is unavailable with the reason `failed` and the engine runs live-only; launch never stops. `LocalSource::start` no longer writes the host row.
- **#3 One writer.** `Store::open` takes a `flock` on `<file>.lock` (`StoreError::Locked`). The app runs as a single instance (`tauri-plugin-single-instance`). Debug builds use the bundle identifier `com.riley.kelvo.dev` (now `com.tryopendata.kelvo.dev`, D-095), so they get their own data directory.
- **#4 Wall-clock steps.** A wall-clock step of more than 2 ticks against the continuous clock is treated like a sleep: flush, reset the buckets, write a `clock_changed` gap, bump `layout_no`. After a step back nothing is persisted until the clock passes the newest bucket already written.
- **#9 Peer identity.** `Hello.host` is a `HostIdentity` without `is_local`. Schema v2 adds a partial unique index for one local host and demotes duplicates. A lost `host-id` file is recovered from the store. Fixtures were regenerated.
- **#13 Row kinds are negotiated** (`rows.buckets`, `rows.gaps`, `rows.events`). Cursors store the kinds they covered, and a resume that negotiates a kind the cursor did not cover resyncs from the start.
- **#16 Typed store errors.** New kinds: `store_busy`, `store_corrupt`, `store_too_new` and `internal`. `history_unavailable.reason` is one of `locked`, `too_new`, `corrupt` or `failed`. New command `reset_history`: it moves the file aside and starts fresh while sampling continues.
- **#19** Pruning and process roll-down yield to queued flushes between batches, and a queued shutdown ends a prune early.
- **#20** A failed commit leaves a `write_failed` gap over the lost span.
- **#21** Pause and engine settings are per host: settings apply only to the local host. A `hosts-changed` event fires when a host record changes. `LiveFeed::now_ms` is left for the live-channel rework.
- **#29** New engine tests: live-only, a failing store, a wake without a sleep, settings changed while asleep, pause/sleep interleavings, the flush before sleep during a long prune, and a store swap.

Verified:
- `make check` passes:
  - Rust: 303 passed, 0 failed, 17 ignored.
  - Vitest: 394 tests in 71 files.
- `make test-fill` passes. 150 series take 139.7 MB. 250 series take 139.9 MB after the cap trim and peak at 150.0 MB right after a prune.
- Revert-verified:
  - clock step forward and back (both the detection and `persist_from`);
  - row-kind gating;
  - flush during a long prune;
  - the `write_failed` gap.

Open:
- The frontend's history banner (`history-state.ts`) still matches only `store` and `history_unavailable`. It should learn the new kinds and `reason`, and offer `reset_history`.

Architecture review fixes: Quit's OS side, the appstore edition, persisted CPU power calibration (D-065).

- Quit and Force Quit read and signal through `kelvo_collect::process_control` (re-exported by `kelvo-engine`), which uses the processes collector's own libproc helpers. The shell's copy of the start-time formula is gone. `info_start_time_matches_the_processes_collector` compares a real collector row for the test process with `ProcessOs::info`. It fails when either formula changes (mutation-checked).
- `appstore` forwards from `kelvo` through `kelvo-engine` to `kelvo-collect`.
  - In that edition `process_signal` answers `unavailable`. The new `get_edition` command reports `{ process_signal: false }`.
  - The responsible-PID `dlsym` is compiled out.
  - `make appstore-check` (part of `make check`) and a macOS CI step run `cargo check -p kelvo --features appstore` and clippy over kelvo, engine and collect.
  - Sandbox gap found, not fixed: the single-instance plugin's `/tmp` socket.
- The SMC-to-PMP scale persists per chip in `power-calibration.json` (app data directory) through a shell-owned `ScaleStore`. New sessions start seeded: the stored scale, else 1.33 on the M3 Max. `power.cpu_source` 3 means seeded. Tests cover seeding, default fallback, implausible stored values, per-window saves and a restart reading the saved scale, plus the file store's round trip and corrupt-file fallback.
- Frontend follow-ups:
  - expose `getEdition` on the transport and hide the Processes actions when `process_signal` is false;
  - label `power.cpu` for `power.cpu_source` 3.
- Verified on HEAD plus these changes in a clean copy, because the parallel live-channel rework did not compile at the time: `cargo fmt --check`, workspace clippy, `make appstore-check`, `cargo test --workspace`, `bun run check`, and the bindings regenerated with `make bindings`' exporter. Calibrated accuracy on hardware is still unverified.

Architecture review fixes: the live channel (D-066).

- `LiveHub` (kelvo-engine) owns each host's ring, latest frame, layout and status; every `Source` publishes through it. The engine's ring, `SourceControl::backfill` and the shell's latest-frame watcher are gone. `LiveFeed::now_ms` is gone: the backfill window is measured from the hub's latest frame, on the source's clock.
- `subscribe_live` is async and takes `series` and `min_period_ms`. It sends the last two minutes before returning and older history as `backfill_earlier` chunks (600 rows, newest first) after the first frame. Layouts, rows and frames are projected in Rust.
- `set_process_interest` takes a `ProcessView { limit, sort[], period_ms }` and the subscription's `stream` id.
  - Top-N per sort key is picked in Rust.
  - The engine samples processes at the shortest period any visible window asks for.
  - Interest tagged with an older stream ends when the page reloads.
- The processes collector skips unreadable pids (rechecked every 60 s, keyed by pid) and reads thread counts every 5 s.
  - libproc calls per tick with a window open: 2,691 before, 1,720 after.
  - Engine CPU with the full table: unchanged, 1.13% in both builds in a parallel run.
  - An Overview-style 5 s period: 37% less engine CPU than the full table.
- Unknown window labels are hidden. A lagging stream catches up from the ring. A clock step back (new `layout_no`, older frame) restarts the stream.
- Proto, additive: `LiveFrame.held`, omitted when empty, and `Message::LiveProcesses`, with fixtures.
- The live tests run on a paused tokio clock. Five mutations each turned a test red: the clock-step restart, lag catch-up, default-hidden labels, and the two stream-token checks.
- Verified: `make check` (331 Rust tests, 394 Vitest), workspace clippy, perf_gates, and `make bindings`.
- Frontend edits: only argument and field fills.
  - `transport.ts` passes `null` for the new arguments.
  - `mock-transport.ts` fills the three new `SubscriptionInfo` fields.
- Frontend follow-ups:
  - prepend `backfill_earlier` chunks (until then the dashboard shows two minutes of history, not an hour);
  - reset live state on a new `layout_no` with an older frame;
  - pass `stream` and a view to `setProcessInterest`: Overview `{limit: 5, sort: [...], period_ms: 2000 to 5000}`, Processes `{limit: null}`, merged per window;
  - pass `series` from popover and widget consumers.

Frontend side of the review fixes (D-064, D-065, D-066, review items #14 and #16).

- Live state:
  - `backfill_earlier` chunks prepend into the row ring and `SeriesColumns`, oldest first, dropping overlap and keeping the newest at capacity;
  - a new `layout_no` with an older frame clears the rows and keeps the subscription;
  - a `rowsEpoch` makes windowed charts recompute when older rows arrive.
- Process interest: a per-window `ProcessInterest` unions every consumer's view (largest limit, shortest period, null wins) and sends it with the current stream id.
  - Overview: `{limit: 5, sort: [cpu, memory, energy, disk_total], period_ms: 5000}`.
  - CPU and Memory cards: top rows by their sort column.
  - Processes and Disk: the full table.
- Series selection: the popover subscribes to its 50 series (`popover/_lib/series.ts`; the wide mock layout has 170). The dashboard subscribes to all, because its routes share one subscription and ring and a per-route projection would resubscribe on every navigation. Popover main-thread time on the default mock: 21.8 ms/s unprojected, 20.3 projected (one run each).
- #14: when frames stop for 3 more intervals while the window is visible and not paused, the store shows `reconnecting` and resubscribes, with backoff up to 60 s. It asks only for the missed span (plus 2 intervals). If the gap is longer than the 2 min recent backfill, it clears the rows and asks for the full window.
- #16:
  - the history banner names the reason (locked, newer format, damaged, failed);
  - "Reset history" opens a confirm that says the old file is kept aside, then calls `reset_history`;
  - it is offered everywhere the banner shows (Settings, Timeline, battery), except when locked or for a plain `store` error;
  - a `store_busy` reset answer is a toast;
  - `store_corrupt` and `store_too_new` are no longer retried.
- Edition: Quit and Force Quit are hidden until `get_edition` says `process_signal` is available. An `unavailable` answer from `process_signal` hides them too.
- `power.cpu_source`: "CPU power uncalibrated" (1, and unknown values) and "CPU power estimated from last calibration" (3) as a muted line on the popover Power card and the Power page's stack card; nothing for 2 or absent.
- The mock implements projection, earlier chunks, process views and streams, `reset_history`, `get_edition`, `hosts-changed`, and clock steps. New scenarios: `history-locked`, `history-corrupt`, `history-too-new`, `appstore`, `clock-step`, `cpu-power-uncalibrated`, `cpu-power-seeded`, `wide-layout`.
- New perf case "dashboard open, backfilling the hour" (`frontend.dashboardOpen` in `perf-budget.json`): on the wide layout with 3,600 rows, no long task over 50 ms from first render until the CPU 1 h line spans the chart. Measured: the line spans the chart about 1.0 to 1.1 s after the page is up, with no long tasks.
- Verification:
  - `bun run check`: 441 Vitest tests;
  - `bun run test:e2e`: 112 passed, 4 WebKit perf cases skipped;
  - screenshots of the popover, Overview, Power and Settings (plus Settings with a damaged file) in both themes.
- Revert-verified:
  - the prepend order and epoch;
  - reconnect;
  - the CPU power note;
  - the reset offer and the reset call;
  - the edition gate and the `unavailable` mark;
  - the perf case, against an 80 ms busy-wait per chunk (6 long tasks) and against a disabled prepend (the line spanned 3.6% of the chart).

#### Idle CPU measured end to end (D-067)

- **Bench.** `make bench` (`scripts/bench-coalition.sh`) measures the packaged app plus its WebKit helpers per scenario (tray, popover, Overview, Processes), opened through `KELVO_BENCH_SCENARIO` in a `bench`-feature build under its own identifier. `make bench-vs-stats` compares tray-only against Stats, or skips if Stats is not installed.
- **Budgets.** `perf-budget.json` has a fixed `coalition.target` (0.5%) and baselines for the coalition (1.41%) and the engine (0.38%). A run more than 10% over its baseline twice fails. Every run prints its distance to the target. `make perf` blocks locally and is advisory in CI, with a notice.
- **perf_gates coverage.** Each Supported collector must take its minimum samples. On real Apple Silicon, IOReport, SMC and HID must be Supported. VMs print `[perf] skipped: no hardware`, which CI surfaces as a notice. Revert-verified: a faked-unsupported `hid.thermal` and a zero-sample `smc.sensors` each fail.
- **Cadence.** `cpu`, `gpu`, `memory`, `disk_io`, `network` and `smc.power` sample every tick only while a window streams or the menu bar shows their module, and every 10 s otherwise (`Interest::Live`). `self.cpu` caches responsible-PID lookups.
- **Before and after.**
  - Engine tray-only, parallel runs: 0.509% to 0.378%, and 0.402% to 0.250%.
  - Coalition tray-only: 1.39 to 1.43% before, 1.37% after.
- **Per-collector engine CPU, tray-only** (`make perf`, 0.329% total):

  | Collector | CPU |
  |---|---|
  | processes | 0.089% |
  | hid.thermal | 0.058% |
  | ioreport | 0.037% |
  | smc.sensors | 0.024% |
  | gpu | 0.022% |
  | battery | 0.013% |
  | cpu | 0.010% |
  | smc.fans | 0.010% |
  | memory | 0.007% |
  | network | 0.007% |
  | self_cpu | 0.003% |
  | disk_io | 0.003% |
  | smc.power | 0.003% |
  | engine core | 0.028% |

- **Open.** The app main thread's tray drawing costs about 0.9 points. 0.5% at 1 s needs cheaper tray redraws, or a 2 s tray or sampling rate; D-067 lists the options. That decision belongs to the user.

#### 5-minute history commits: write volume and the right edge (D-070)

- **Commit interval.** `DEFAULT_COMMIT_INTERVAL` is 5 minutes (user decision: fewer disk writes over a shorter crash-loss window). The engine flushes on wake, before sleep, on a store swap and at shutdown.
- **Measured WAL volume** (real collectors, 170 series, tray-only at 1 s, an hour of fake time). At 30 s: 120 commits/h and 19.1 MB/h appended to the WAL. At 300 s: 12 commits/h and 3.9 MB/h. Counted inside the writer with `SQLITE_DBSTATUS_CACHE_WRITE` (feature `write-stats`, test builds only). The perf gate prints commits and WAL bytes per hour (4.04 MB/h on its 10-minute run) and asserts `engine.store.walBytesPerHour` once the budget has it. `wal_volume_by_commit_interval` (ignored) reruns the comparison.
- **Frontend right edge.**
  - The Timeline redraws the ring-only span when earlier chunks or a late backfill land.
  - `useProcessesAt` asks again for a bucket whose answer was fetched before it could have been committed.
  - The battery bars' running hour follows the current reading and refetches every commit interval.
  - Each fix has a hook test, and each test was revert-verified.
- **Open.**
  - The Overview 24 h bar scales ignore a spike in the uncommitted minutes. They were already up to 10 minutes behind.
  - The "last wake" 10-minute cache can still show the previous wake.
  - A ring merge in `query_history` would fix all of these at once (D-070).

#### Cheaper tray redraws (D-073)

- **Where the main thread went.** `sample` of the tray-only bench app: every drawn frame paid tray-icon's PNG encode and decode on the main thread, two `_adjustLength` relayouts (the image arrived at pixel size and was shrunk after), and a second dispatch for the accessibility label. The rest is AppKit redrawing the button, updating the status item scene and its replicants. The frame skip already covered the real path (23 to 50% of ticks skipped; nothing dispatched for a skipped frame).
- **Change.** Same-size frames go straight into an `NSBitmapImageRep` template image at its point size, with the label only when its words change, in one main-thread call, into a status item whose length is pinned. A size change goes through tray-icon as before and pins the new width. `model::ItemState` decides; its test was revert-verified twice (resize always, label always).
- **Measured.** Parallel tray-only runs of HEAD and the change (same settings, same load, 120 s): main thread 0.259 to 0.199%, 0.321 to 0.258%, 0.135 to 0.100% (20 to 26% less); coalition 0.562 to 0.505%, 0.663 to 0.604%, 0.355 to 0.330%. Alternating `make bench` runs swing 0.4 points with load and do not resolve the change. `coalition.baseline` is now 1.01.
- **Open.** 0.5% at 1 s is not reliably met: 0.33 to 0.60% in parallel runs, 0.68 to 1.0% alternating. Each drawn frame still costs the main thread 5 to 8 ms inside AppKit. The remaining options and their estimated gains are in D-073; they change the cadence or a default, so they are the user's call.

#### CSP, opener scope, gap bands on module pages, CPU power label

- **CSP.** `tauri.conf.json` ships `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'`. The bundle has no eval, `new Function` or inline script. `'unsafe-inline'` styles are needed: Radix (scroll area, select), react-remove-scroll, sonner and React 19 insert `<style>` elements at runtime (removing it gives `style-src-elem` violations). `dangerousDisableAssetCspModification: ["style-src"]` keeps Tauri from adding a nonce to `style-src`, which would silently turn `'unsafe-inline'` off. Vite inlines one small font subset as a `data:` URL, hence `font-src data:`. On desktop Tauri sets the CSP only on bundled assets, so `bun run tauri dev` (Vite on :1420, HMR websocket) runs without it and needs no `devCsp`.
  - `tests/e2e/csp.spec.ts` builds the frontend, serves it from a fake origin with the shipped policy as a header, and walks the dashboard pages, the popover, onboarding, a Radix dialog and select with no violations; it also checks an injected inline script is blocked. Outside Tauri the bundle runs the mock transport, so the `ipc:` directives are only exercised by a built app: unverified until a `bun run tauri build` run.
- **Opener scope.** The webview opens one URL: the sensor dump issue (`opener:allow-open-url` scoped to `https://github.com/tryopendata/kelvo/issues/new?labels=sensors&title=*`). `opener:default` (any http, https, mailto, tel URL plus reveal-in-Finder) is gone. `opener-scope.test.ts` checks the capability against the plugin's glob semantics; a refused call in the real app is not tested (no WKWebView in Playwright).
- **Gap bands on module pages (#17).** `useGapBands(module)` fetches the gaps over the ring's hour once per page (`historyKeys.ringGaps`, refetched every minute and on pause or resume) and hands them to StreamArea, PowerStack, CoreHeatmap and LiveMirrorChart, which clip them to their own axis. CPU, GPU, Memory, Network, Disk and Power draw them; the battery 24 h bars keep their hatched hours. The mock ring now has no rows inside a closed history gap and its history gaps no longer slide with the clock.
  - Compared with board 12 (sleep gap, 1h): same hatch, dashed edges and label pill. Left in place: the pill straddles the plot's bottom edge, so it covers part of the x-tick row under it (the board's lane has 12 px more room there); a band reaches the chart up to a minute after a wake (the refetch period).
- **CPU power label (D-054).** The note under CPU power (popover Power card, Power stack) now says "CPU power: P cores" whenever `power.cpu_source` is present, plus ", uncalibrated" (1) or ", estimated from last calibration" (3). The Cluster frequency card says "Not measured" for a cluster whose power series is not in the layout (the M3 Max E cluster). New mock scenario `cpu-power-calibrated`; the SMC scenarios drop the E-cluster power series. Overview and Timeline still show `power.cpu` without the note.
- Verified: `bun run check`, the CSP spec and the gap-band e2e checks in both browsers, `cargo check -p kelvo` (capability and config parse). Each new regression test was revert-verified.

#### Perf gates and dependency audit (D-075, review #15)

- **`make perf`.** The 1 s and 30 s engines run one at a time. A run that ticked under 90% of the expected count exits 2 without a verdict. `engine.perf.regressionPct` is 30, because identical code measured 0.159 to 0.222% across seven runs in one sitting and 0.15 to 0.38% across sittings. The gate catches large regressions only; small ones still need parallel same-sitting runs.
- **Flaky processes allocation gate.** The count followed how many processes the machine started inside the 2 s fake-ticker window, at three allocations per new process name, plus a regrow whenever a sample had more rows than the last. Now one `Arc<str>` per new name (`name_of` borrows), and `take_processes` leaves an eighth of headroom. Window-open went from 0.18 to 1.63 per tick to 0.17 to 0.42 under the same build load. The limit stays 1. The gate prints new process rows.
- **Budgets.** `engine.store.walBytesPerHour` is 5,000,000 and asserted. The hidden-resume Playwright case has its own `frontend.hiddenResume.longTaskMs`.
- **Dependency audit.** `deny.toml` (advisories, licenses, bans, sources). CI jobs `deny` (cargo-deny-action; advisories non-blocking) and `audit-frontend` (`bun audit`, non-blocking). `make deny` skips when cargo-deny is not installed. cargo-deny has not run yet; the first CI run is its first check.

#### 15-minute history beyond 7 days (D-076)

- **Store.** Schema v3 adds `tier_15m` and `proc_top_15m`. The prune keeps 7 days of minutes and rolls older ones into 15-minute rows: min of mins, max of maxes, the mean of sampled minutes, NaN where none had a value, and one row per layout. `proc_top_1m` rolls into a top 5 per 15 minutes. `Auto` reads M15 for ranges that start before the roll cut, and an M15 read folds in the remaining minutes. The cap trims 15-minute rows first, on 15-minute boundaries.
- **Sync.** A new row kind, `rows.m15`, with its own cursor. A receiver without it gets `Truncated` at the roll cut when its minute cursor missed rolled rows. Old builds decode `"m15"` as `Unknown`. New fixtures `sync_request_m15` and `sync_page_m15` were added and the existing fixtures are unchanged.
- **Sizes** (`make test-fill`): 150 series 139.7 to 69.9 MB, 250 series 139.9 MB (capped) to 95.7 MB (uncapped); with D-089 per-app network history 89.2 MB and 115.0 MB. A third run with a 111 MB cap keeps the trim exercised.
- **Frontend follow-ups.**
  - Done in d496112: the Timeline 24h range requests `auto`, `TIER_MS` has `m15`, and the bucket width and tooltip resolution follow the returned tier, so a day older than 7 days draws from `tier_15m`. The mock mirrors `Auto`.
  - Done in d496112: `history-projection.ts` is refitted to the D-057 and D-076 fill pairs (a quarter day costs a fifteenth of a minute day). 90 days at 150 series is about 83 MB.
  - `battery-hours.ts` stays on `m1` on purpose: its range is the last 24 hours.
  - Done in v1.1 (f79cd01): a heatmap click on a day older than 7 days opens 6 h centred on the hour.
- Verified with new tests in `tests/rolldown.rs` (fold, layout split, NaN, gaps, Auto, width weighting, process roll-down, retention), a sync test for both kinds of controller, a v2 to v3 migration test, a cap-through-M15 test and the pre-M15 decoder pin. The fold's NaN, the width weighting, Auto's M15 choice and the `rows.m15` truncation guard were each revert-verified.

#### Tray redraws at most every 2 s (D-077)

- `Pacer` replaces `FrameSkip`. It skips equal frames, draws a changed frame at most every 2 s (4 s while backed off), and holds the newest changed frame in between. A timer on the tray thread's own current-thread runtime draws the held frame 0.5 s after the period if no newer frame came. Pausing draws at once; display idle drops the held frame. Sampling stays at 1 s.
- Unit tests in `tray/model.rs`:
  - equal-frame skip;
  - at most one draw per period with 1 s jittered frames, and the timer never fires while frames flow;
  - a held frame drawn at its deadline and cancelled by a frame back to what is shown;
  - urgent draws, and the doubled period when backed off.
  - Revert-verified twice: with the throttle removed (3 tests red) and with the held frame never drawn (the deadline test red).
- Not benchmarked with `make bench`; the user called performance settled. Five parallel pairs (D-073 method) ran before that call:
  - the main thread fell 0.06 to 0.29 points in every pair;
  - the coalition read 0.46 to 0.69% after against 0.52 to 1.0% before, under load average 24 to 152.
  - `perf-budget.json` is unchanged.

#### Architecture review 3 fixes

The third architecture review covered D-068 to D-078 and the commits since `3e25e5b`. Fixes in 9e6e082, f9137e7 and c35cc69, plus the follow-ups noted below.

- **Flushes on pause, resume and module switches (D-070).** With 5-minute commits, an open `paused` or `module_disabled` gap stayed in the batch, so readers drew it over new samples for up to 5 minutes. The engine now flushes on pause, on resume, on a wake while paused and on a module switch. Tests: `pause_and_resume_reach_readers_without_a_flush`, `module_switches_reach_readers_without_a_flush`.
- **Gaps reopened after a long backward step.** The clock-step discard deleted the `module_disabled` and `paused` gaps the engine still held open. They are opened again and committed after the discard. Test: `a_long_backward_step_keeps_a_disabled_module_gapped`.
- **`discard_from` commits on its own and is retried.** It was queued, so a failed commit lost it silently and the new clock upserted into the old clock's rows. It is now a replied call that commits the batch before it and then itself; the engine keeps the hold until the store confirms it, so the hold can outlast an hour. Tests: `discard_from_commits_before_it_returns`, `a_discard_that_fails_to_commit_is_reported_and_can_be_retried`, and on the engine side `a_failed_discard_holds_history_until_a_retry_succeeds` (commits fail through the hold via `Writer::set_fail_commits`, behind the `write-stats` test feature).
- **`auto_tier` slack.** A 7d range ending now flipped to `tier_15m` depending on when the last prune ran. M15 is picked only when more than one 15-minute bucket of the range was rolled down. Test: `a_range_starting_less_than_a_quarter_before_the_roll_cut_reads_minutes`.
- **Host identity in `kelvo-engine` (D-071, v4).** `load_or_create`, the host-id file and clone detection moved from the shell to `kelvo_engine::identity`, so the v4 agent derives the same id. `IOPlatformUUID` is read through `kelvo_collect::macos::platform_uuid`. Tests: `the_binding_hash_is_stable` (golden hash), `this_mac_has_a_binding`.
- **Tray redraws after a failed draw.** A failed render or `set_icon` left the Pacer thinking the frame was shown, so equal frames after it were skipped and the icon stayed stale. `Pacer::forget_last` drops it. Test: `pacer_redraws_after_a_failed_draw`.
- **`processesAt` key and debounce.** The key is now (host, bucket start, bucket width), since 10 s, 1 min and 15 min buckets can share a start. Start and width are debounced together, so a range switch never asks for the old start at the new width. Tests in `use-timeline-data.test.tsx`: "asks again when a range switch changes the bucket width at the same start", "never asks for the old start at the new width while a range switch settles".
- **Gap band keys.** `GapBands` keyed bands on the start alone; two gaps starting in the same millisecond collided. The key is start, end and label. Test in `gap-band.test.tsx`: "keeps two gaps that start in the same millisecond apart".
- **Doc corrections.** The D-076 frontend follow-ups are marked done in d496112. D-070 is amended for f9137e7. architecture.md and v1-local-monitor.md now say 5-minute commits with the full flush list, and that clock steps are an explicit `timeline` (D-072). A new known follow-up: `history_held_until` reaches neither IPC nor the UI.
- **Clock-step discard edge cases** (857e73a, 0d093f5, 710b116). After a confirmed discard the engine rewrites the step's `clock_changed` gap out to the retry, so a discard that kept failing past the hold leaves no unmarked span; a later step while the discard is pending widens that gap instead of replacing it, and keeps the hold. Tests: `a_step_while_a_discard_is_pending_keeps_its_gap`, `a_forward_step_during_a_pending_discard_widens_the_clock_gap`, `a_small_backward_step_keeps_the_hold_of_a_pending_discard`.
- Each new regression test was revert-verified. Verified: `make check` passes.

#### v1.1: long-range Timeline, heatmap, CSV export, tray styles

- **7d and 30d Timeline (1.1-A, D-079).** Every span asks for tier `auto`; 7d reads `tier_1m`, 30d `tier_15m` (D-076). The frontend picks a slot width from a fixed ladder at about 2 points per plot pixel and aligns the start to it; `HistoryPage.bucket_ms` is the width each point averages over and drives the tooltip label ("10 min avg"). Back and forward step one range; the subtitle shows both ends when not Live. Lanes draw the min–max envelope behind the average. 7d ticks at local midnight, 30d on Mondays. The mock has 29 nights of sleep plus a 34 h away gap, an app-off afternoon and a paused afternoon.
- **Heatmap (1.1-B).** `query_heatmap` takes 25 UTC hour boundaries per local day from the frontend (`heatmapDays`), so the store stays tz-free and DST days come out as 23 or 25 hour rows; 15-minute buckets weigh 15 and minutes 1. `CalendarHeatmap` paints one canvas (board 06 alpha formula, hatched empty hours, today's row, the current hour outlined, future hours plain and disabled, a neutral loading state), a grid of accessible cells over it, Avg CPU / Temperature (40 to 90 °C). It refetches every 5 minutes and when the local hour changes, never per tick. A click opens that hour at 1h; an hour older than 7 days, or a 2-hour fall-back cell, opens 6 h centred on it (a span with no preset; stepping onto now returns to Live 24h).
- **CSV export (1.1-C).** `export_csv` streams from the store inside one read transaction, flushes the writer first so a Live export includes the uncommitted tail, and writes a sibling temp file that is synced and renamed over the target, so a failure keeps a replaced file. Columns: `time_utc,time_ms,<series>_avg,_min,_max…,gap_reason,gap_end_ms`. Rust opens the save sheet (`tauri-plugin-dialog`). The Timeline header button exports the visible range and lanes.
- **Tray styles and own items (1.1-D, D-080).** `MenuBarMode` gains `OwnGraph`, `OwnValue` and `OwnCores`. Each own-item module gets its own status item with an `autosaveName` (`kelvo-cpu`, …, D-037). Graphs: CPU sparkline, GPU history bars, memory gauge, network rates; Cores: per-core strip with a cluster gap. All items share one D-077 pacer and one main-thread draw per period. Settings offers the new modes; onboarding has the Graph per module card.
- **Measured.** `make test-read-perf` (150-series 30-day fill, release): 7d six lanes 34.5 ms, 30d 38.6 ms (budget 500 ms), heatmap 4.1 ms, 30-day export of all series 75.1 ms (budget 3 s). Tray coalition with own items (D-080, bench under heavy load): Combined 0.81 to 0.92%, Graph per module 1.0 to 2.0%, every module own 1.8 to 2.4% of a core; the added cost is main-thread AppKit redraws of items that change every 2 s.
- **Open, moved to manual QA:** ⌘-drag order persistence across launches (needs a human), and the WindowServer re-measure (WindowServer read 20 to 37% of a core under the parallel agent load, so 0.2% added could not be resolved). The tray-budget acceptance item is therefore not met for own-item configurations as measured; Combined, the default, is unchanged.
- Board 06 differences left in place: day labels use muted-foreground for contrast, the hatch uses the grid token, the metric toggle is the shared segmented control, 7d/30d tick and peak formats are new (the board draws only 24h), and the 30d lanes omit the Sleep/Wake marker row. Board 01: network rates at 8 pt on a 9 pt pitch (the board's 9 px clips), core strip 44 pt wide.
- Verified: `make check` and `bun run test:e2e` pass; v1.1 code review fixes re-reviewed. Every new regression test was revert-verified.

#### v1.2-A: per-process network

- **Collector (D-081, D-082).** `kelvo-collect/src/macos/nstat.rs` loads NetworkStatistics with dlopen once and creates an NStatManager only while it is sampled (`Cadence::OnDemand(NetworkProcesses)`, `Entitlement::NetworkStatistics`, absent from the appstore build). Each query waits on a completion block with a 250 ms timeout. Flows that existed before the manager report no owner until described, so the first sample and any sample with unresolved flows also run a description query (at most every 10 s). A ledger keeps per-flow bytes, folds closed flows into a per-pid accumulator, skips loopback, and the first tick only sets the baseline. Rows cover the user's own processes; there is no remainder row.
- **Engine.** `Interest::NetworkProcesses` is held while a visible view asks for it (registry aggregates over windows); the manager is released (`Collector::release`) as soon as it is not. Rates merge into `ProcessSample` by pid; a process with no flows gets 0. `Capabilities.process_network` follows the collector's probe. `ProcessSort` gains `NetRx`, `NetTx`, `NetTotal`.
- **UI.** Overview Network card shows the top 5 processes ("Your processes only"); the Network page gets a process table; the Processes page gets ↓/↑ columns. All hidden when the capability is absent, and the interface list stays. Mock: deterministic rates and a `no-process-network` scenario.
- **Measured.** perf_gates: trayOnly and window 0 nstat calls per tick; windowNetwork 1.00 call per tick, 0 collector allocations, engine core allocations unchanged (9.02). `dump --perf` (release, 120 s, heavy load): Overview-style window 0.537% vs 0.607% with network rates; the collector itself 0.0025%. One query 0.6 to 1.3 ms; create plus baseline about 3 ms. Live check against `nettop -P -d` (stand-in for Activity Monitor): a 6 MB/s-limited curl read 6.22 MB/s in Kelvo, 6.33 MB/s in nettop over the same window.
- Board 04/05 differences left in place: the "Your processes only" label (one line taller), process names are executable names ("com.docker.backend", not "Docker"), rates use the app formatter ("700 KB/s", not "0.7 MB/s"), the subtitle shows "en0" without the interface type (pre-existing).
- Verified: `make check` and `bun run test:e2e` pass; every new regression test was revert-verified.

#### v1.2-B: per-process GPU

- **Collector (D-085).** `kelvo-collect/src/macos/gpu_procs.rs` walks each `IOAccelerator`'s user clients (unregistered children, so the child iterator), reads the pid from `IOUserClientCreator` and sums `AppUsage` `accumulatedGPUTime`. Deltas are keyed on registry entry id and summed per pid; a share is GPU ns over wall ns, percent of the whole GPU. Each process is clamped to 100% and all are scaled down when the sum exceeds 100%. `Cadence::OnDemand(GpuProcesses)`, `Entitlement::IoRegistryGpuClients`; first sample a baseline; `release` forgets everything. No Rust allocation per pass. `iokit.rs` gains `children()` and `registry_id()`.
- **Engine and UI.** Same shape as 1.2-A: `ProcessView.gpu`, `set_gpu_process_interest`, `Capabilities.process_gpu`, `ProcessSort::Gpu`. The Overview GPU card switches from the 60 s chart to the top 5 processes ("Your processes only"); the GPU page gets a Processes table (process, PID, % GPU, user) with a note that shares are approximate under long compute jobs; the Processes page gets a GPU column set (its own set, so the IOKit walk only runs when chosen). All hidden without the capability. Mock: board 04's shares, `no-process-gpu` scenario.
- **Measured.** One pass over 90 clients: 1.1 to 1.2 ms, 211 IOKit calls. perf_gates: trayOnly and window 0 samples; windowGpu 216 iokit calls per tick, 0 collector allocations, engine core 9.02 (same as window). `dump --perf` (release, 120 s): collector 0.027% of a core at the Overview's 5 s rows, 0.148% at every tick. Live check against a Metal load's own busy time (Activity Monitor could not be read non-interactively): 96.6% vs 97.4% (9 ms buffers), 39.8% vs 37.7% (half load); 97.7% vs 76.7% with 0.64 ms buffers (the counter includes per-buffer overhead); with 1.86 s buffers in a 1 s window the load read 0% and the clients queued behind it were charged, the documented caveat.
- Board 04/05 differences left in place: the "Your processes only" line (one line taller), executable names ("com.docker.backend"), the pre-existing "Tiler" legend and empty core-count subtitle; the mock's WindowServer row is a mock liberty (real rows are the user's processes).
- Verified: `make check` passes; `bun run test:e2e` passes except the perf-gate spec's long-task checks (one 53 to 55 ms task on the CPU or dashboard-open test) while parallel agents held the load average near 12; the perf-gate spec passes on its own. Every new regression test was revert-verified (7 Rust, 15 frontend). The `windowNetwork` gate also caught an occasional NetworkStatistics map regrowth (0.02 allocations per tick); the ledger now reserves its room when the manager opens.

#### v1.2-C and 1.2-D: detectors, annotations, first alerts

- **Detectors (D-083).** `kelvo-engine/src/detect/`: fans ramped, thermal state, sustained process, package and ANE power spikes, thresholds in `DetectorThresholds::DEFAULT` (`kelvo-schema`). They run on the persisted tick, reset over sleep, pause, clock steps and store changes, and attribute from the processes collector's existing batches (no forced sampling). Events are CBOR `events` rows; `Writer::commit_soon` makes them readable at once, and `BusMsg::Event` becomes the `event-recorded` Tauri event. `query_events` returns them. A 25-minute real recording (`crates/kelvo-engine/tests/fixtures/busy-25min.json`) changed the fan detector from a plain refractory period to a re-arm after the fans settle, with a 500 rpm floor for stopped fans.
- **Timeline and Power.** Event pills (board 06) above the lanes with an amber band per episode, laid out apart from Sleep/Wake so neither hides the other; clicking a pill puts the crosshair on it and focuses the cursor. `useEvents` reads once and merges pushed events into every cached list. The Power chart marks `power_spike` events (board 08). The mock transport has a fixed day of events (14:02 fan ramp with the board's clock) and `recordEvent`.
- **Alerts (D-084).** Two rules, off by default, Settings "Alerts" rows, a 30-minute cooldown per rule, `alert` events, notifications through tauri-plugin-notification. In `tauri dev` the plugin posts as Terminal (macOS asked for Terminal's permission). Clicking a notification does nothing on desktop; opening the Timeline needs `UNUserNotificationCenter`.
- **Not built:** the optimized-charging pill (no documented source, D-083) and notification click routing (D-084).
- **Board differences left in place.** Board 06: pills take their own rows and Sleep/Wake sit on a row under them, where the board lets the pill overlap the markers; the board's "10:12 Docker pull 1.8 GB" network marker has no detector, so the mock's 10:12 event is a sustained Docker process; pill widths are estimated, so the row layout can leave a little more space than needed. Board 08: the battery chart has no optimized-charging pill.
- Verified: `make check`, Rust unit and integration tests (detector fixtures including the recorded one, store, engine events, perf gate 0 allocations per tick), Vitest, Playwright (new `timeline-events.spec.ts`, both themes and engines). Every new regression test was revert-verified or mutation-tested. The perf gate passes run alone; in the parallel full run under load average 13 to 20 it reported 51 to 54 ms long tasks on the Overview and CPU pages, which this work does not touch.

#### v1.2 code review fixes

Two review rounds after the v1.2 merge into the worktree branch; no Critical findings. Fixed:
- **NetworkStatistics session (D-081, D-082 amended).** Each query waits for the first completion after it was issued, so a lost completion cannot stall the session; a timeout drops the session and the next sample rebuilds it; three failures in a row back off for 60 s (`CollectError::Timeout`). The manager is destroyed on its own serial queue (`dispatch_sync_f`), so queued callbacks finish first; an ignored live churn test ran 2,000 open and release cycles under loopback traffic with no timeouts or crashes. A flow whose owner is learned late counts from that moment instead of producing a one-sample spike.
- **GPU ledger (D-085 amended).** Idle clients are recorded so a client's first interval of use counts; a registry change during the walk retries it once; a cut pass never primes a fresh ledger and keeps missed clients for one pass only; a missing child iterator fails the pass and restarts from a baseline. `windowGpu` now makes about 277 IOKit calls per tick (ceiling 350).
- **Engine and detectors (D-083, D-084 amended).** On-demand collectors are released on pause and will-sleep. `power_spike` needs 10 readings to warm up and treats a hole of more than three of the series' own sampling periods as a restart (the first version used the base tick, which stopped it ever firing tray-only; caught in the second round). Fans re-arm 10 minutes after a ramp; `sustained_process` reports a name at most every 10 minutes (a scripted cargo build: 28 pills before, 3 after). Alert cooldowns survive toggles and restarts (seeded from stored `alert` events); a rule whose condition never cleared stays quiet; one hot-process alert names every process that completed together; switching a rule on posts an "Alerts are on" notification so macOS asks for permission then.
- **Frontend.** Pushed events go into a 10 s per-host buffer that every `query_events` answer merges, so an event published before its commit lands is not lost to a read in between. The Network and GPU process tables only grow while mounted (`ProcessTable growOnly`), so they do not change height at 1 Hz.
- **Perf-gate failures in parallel runs were load, not v1.2.** Alternating full e2e runs: v1.1's tip failed the same long-task checks under load average 10 to 19, and both tips passed when the machine was quiet (cpu1h 16.6 ms/s quiet vs 26 to 27 ms/s loaded). Most of the near-threshold work is React dev-mode element creation in the CPU page's core heatmap.
- **Decision numbers.** The tray decision written as D-079 became D-080 and the GPU decision written as D-083 became D-085 when parallel lanes collided; older commit messages still use the first numbers (rewriting them was refused by the permission check and not pursued).
- A third, narrow review round fixed alert resets (a relayout, clock step, sleep or toggle now holds off only an episode already reported, and the process rule only the names it reported). The Network and GPU process cards keep their table mounted, so before the first batch they show the header with a "Measuring…" row instead of a bare line (no board covers this state).
- Verified: `make check` (630 Vitest tests, Rust suites including perf gates) and `bun run test:e2e` (191 passed, 5 skipped) pass on the merged tip; every new regression test was revert-verified.
- Housekeeping after the merge: the session's worktrees and their branches are removed (all merged into `main`); the decisions index now lists D-048 to D-085 (D-068 and D-069 were never used); v1.1 and v1.2 acceptance status is recorded under each phase in `v1-local-monitor.md`.

#### Motion, PR A: dashboard choreography, press feedback, crossfades (D-086)

- One-shot motion borrowed from opendata, CSS only, in `src/app/lib/motion/`. Dashboard pages lift in section by section on every navigation (Overview's cards one by one), onboarding steps the same, the sidebar selection is a sliding pill, buttons darken on press, segmented options and the switch thumb squeeze, the status pill and the memory pressure badge fade in on a state change, the stale dim fades, dialogs and menus use a softer zoom.
- Motion tokens are split by cost: `--motion-tick` for per-tick tweens (the only token power saver zeros) and one-shot tokens that stay on battery. `--ease-tick` is a gentler curve for the tick tweens. `motion.md` and design-system.md "Motion" are rewritten for the new policy.
- Verified: Vitest (new primitive and sidebar tests, the pill test revert-verified), `tests/e2e/motion.spec.ts` in Chromium and WebKit (entrances run and settle with nothing running or invisible, reduced motion animates nothing, power saver keeps one-shot motion), `perf-gate.spec.ts` within noise of main (numbers in D-086). Settled screens are unchanged against boards 04, 07 and 15; frozen mid-flight frames checked by eye.
- Next: PR B (popover open choreography with a replay policy, after a stale-frame check in `tauri dev`), then PR C (longer tick step and a leading-edge marker, measured with `make bench` before and after).

#### Motion: headline number ticker (D-087)

- `NumberTicker` counts ring centre values and stat-strip figures to their next value when they move 10% or more or change unit; "10 GB" to "100 MB" counts down through the megabytes. Ported from opendata's `createTween` (`src/core/tween.ts`) and `RollingNumber` (`parseFigure`/`figureAt`/`tickPath` in `src/core/format/figure.ts`). Writes text from rAF without React renders, over `--motion-count` (450 ms, off in power saver and reduced motion).
- Verified: Vitest (figure parsing and log-space counting across units, the component's count, retarget, land-in-place and token-off paths), `tests/e2e/motion.spec.ts` (power saver zeros `--motion-count`), `perf-gate.spec.ts` with counting on and off in the same sitting (Overview median +1.4 ms/s, numbers in D-087). Settled text is the formatter's exact output, so settled screens are unchanged. `make bench` not run.

#### Performance mode (D-088, in progress)

- `sampling.performance_mode`, plus macOS Low Power Mode, resolve in the engine to `PerformanceReason` (the setting wins). When on: processes every 30 s and temperatures every 10 s in the background (temperatures only when the menu bar shows none), visible frames and process rows at most every 2 s, battery back-off forced, tray redraw 4 s (8 s backed off), every motion token 0 through `data-performance`. `power_saver` / `data-power-saver` are gone: being on battery alone no longer turns motion off.
- Fewer samples, not gaps: the process snapshot reader tolerance is 15 s (`ProcResolution::Snapshot`), detector attribution falls back to the newest batch within one idle period, and live charts grid on the frame period (`gridIntervalMs`).
- UI: Settings > Sampling row with an always-shown list of what changes for this user (`src/core/performance.ts`), the battery row held, a sidebar footer line and a "2s · perf" popover pill, both with a hovercard saying why. Divergence from boards 13 to 15 is by design (user call).
- `make bench-perf-mode` (`scripts/bench-perf-mode.sh`): off/on in parallel pairs on two bundle identifiers, in-memory overrides only; refuses to run on battery or in Low Power Mode.
- Verified: `cargo test --workspace` (527), clippy, appstore check, Vitest (665), Playwright (207 incl. `performance-mode.spec.ts`), one code review round fixed. `rust-linux-check` cannot build bundled SQLite for Linux on this Mac; schema and collect cross-check clean.
- Open: Phase 0 bench numbers and `minSavingPp`; `perf-budget.json` visible and performanceMode keys, advisory visible verdicts in `bench-coalition.sh`, the `perf_gates` trayOnlyPerformance mode; docs and the D-088 entry; the Timeline tooltip does not say how far away a 30 s snapshot is.
#### Network attribution: which app caused that spike (D-089)

- Per-app network bytes are now recorded always-on at the 10 s tray cadence and persisted in `proc_net_10s/1m/15m` (schema v4). A one-hour in-memory ring is merged with the store, so the newest five minutes are queryable before the commit. A "Network history" setting (Settings > Sampling, default on) turns it off and falls back to D-082's on-demand rates.
- App identity follows the D-089 rule: XPC services go to their responsible app, then the outermost `.app` (not Xcode toolchains or framework Python), then the argv[0] basename, with a `git` alias. It's captured when the flow's counts first arrive, keyed by `uniqueProcessID`, so a short-lived `curl` keeps its name. Fresh flows named late keep their bytes in history, but not in the rate.
- `query_network_by_app` returns per-app bytes plus "Other apps", "Protocol overhead (est.)" (interface packets × 66 B) and "System and other". It also returns `complete_to_ms`, so the UI never treats an open bucket as final.
- The Network page has a brush on the throughput chart (drag, click a 10 s bucket, keyboard, pinned after scrolling off) and an Apps table in bytes with a "now" column. It matches board 16 except for the differences named in the frontend commit.
- Verified:
  - `make check`, and `bun run test:e2e` (203 passed, 5 skipped).
  - Live `live_net` test, tray-only on real collectors, 3 runs: a 5 MB/s 30 s curl within +0.9% to +2.7% of curl's count, a short curl named after it exited, the ring answering before commit, and apps + overhead + system equal to the interface total.
  - Coalition bench, 6 alternating tray pairs on AC: whole app +0.037 pp, GCD + engine threads +0.036 pp, wakeups −0.15/s. That passes D-089's ≤0.10 pp / ≤0.5/s criterion.
  - Three code review rounds fixed.
- Left open:
  - Chrome and Safari downloads weren't shown live (browser download prompts blocked the scripted downloads; Safari traffic was attributed to "Safari"). Check them during QA.
  - The "now" column can't tell an exited app from an idle one.
  - A helper that exits before its identity resolves shows under its truncated process name ("Google Chrome He").

#### Sample holds and metric kinds (D-090, in progress)

- Fixes the power stack's gaps and lone dots: adaptive collectors (IOReport, CPU, network, disk) sample every 10 s with only the tray open, and the chart broke on every missing slot. The catalog now marks span averages (`Mean`, `Rate`), the engine publishes each series' hold, and `seriesWindow` joins samples within it. Rule: `.claude/rules/data-boundary.md`.
- Verified: cargo test, clippy, perf_gates allocs; Vitest 728; the mock's Power page draws a continuous 10-minute stack (10 s steps over tray-only history).
- Left open: check the real app over tray-only history; the remaining client-side derivations found by the audit (engine constants mirrored in TS, client rollups and derived metrics) need new catalog metrics, a one-way door.

#### Charts across a sampling interval change; one chart window (D-091)

- Plugging in or unplugging changes the sampling interval (battery back-off). Live charts used to lay the old rows on the new grid, so 2 s history on a 1 s grid broke into single points marked with gap dots, and duplicate React keys left stale dots behind. Rows kept the spacing they were sampled at (`f63db47`; D-090 since replaced that with published holds), and StreamArea draws one dot for a one-point run.
- `general.chart_window` (5m/15m/30m/1h, default 15m) is one saved window shared by CPU, GPU, Memory, Power (new selector), Network and Disk. Every time-based card on those pages follows it: the per-core heatmap (about 60 columns), GPU power and frequency residency, swap, the power stack, and zone min/max. Overview, the popover and Timeline are unchanged. At slow sampling the page shows the shortest allowed window without rewriting the saved one.
- Board differences, by decision: boards 07 and 16 show 1m/5m/15m/1h with 1m and 5m pressed; the pages offer 5m/15m/30m/1h at 15m. Board 08 has no window control; Power now has one in its header. Everything else on CPU (15m, 1h), Power (30m, 1h) and Network matched in screenshots.
- Each module route waits for settings once and then renders its whole body, so the page appears in one step without reflow. If `get_settings` fails, the pages fall back to 15m instead of staying empty.
- Verified: `make check` (Rust 605, Vitest 751), `bun run test:e2e` (214 passed, 8 skipped), the new `chart-window.spec.ts`, and the perf gate at 1h: CPU 17 to 19, GPU 13 to 15, Power 16 to 18 main-thread ms/s at 1 s sampling. At 0.5 s, GPU measured 21 and Power 26, all under the 35 budget with no long tasks. One code review round fixed.
- Left open: run the real-app relaunch check (pick 30m, quit, relaunch, `settings.json` has `"chart_window": "30m"`).
- Switching the window refits autoscaled ceilings to the new window's data at once (`useNiceCeiling` takes `windowMs`); the 60 s shrink hysteresis only applies while one window streams. Covers the Network and Disk mirror charts, the power stack, GPU power, swap and the popover CPU chart.
- Network and Disk mirror charts have a hover tooltip (`mirror-hover.tsx`): the bar's time span, both sides' bucket averages, and the gap reason over an empty bar. It writes the DOM through refs like `BrushOverlay`, so hovering re-renders nothing (the brush render-count test covers it). No board draws this tooltip; it borrows board 06's crosshair tooltip styling.
- Settings overhead sentence counts only `self.cpu` readings taken since the sampling setup last changed (interval, frame period, Performance mode, pause, display idle; `statusSinceMs` in the live store), skips the first reading after a change because it spans it, and says "Measuring Kelvo's CPU at 30s…" until two more arrive. It names the interval in effect and that the open window is included. The mock's `self.cpu` now samples every 10 s or every tick, whichever is slower, as the engine does, and its cost scales with the tick rate.
- History size projection uses this Mac's measured growth once an hour is recorded: `history_growth` (kelvo-store `Reader::history_growth`) costs each table by the bytes its rows use at the page fill the larger tables show, scaled to its retention over the minutes actually recorded. On the dev Mac's database (105 series, about 13.5 h recorded) it gives 36.5 MB fixed and 2.05 MB per minute day, so 30 days is about 54 MB where the fill-test model said about 80. Onboarding still uses the fill-test model. Board 13 says "Kelvo uses about 0.4% CPU at 1s"; the page adds ", this window included".

#### Data-boundary follow-ups (D-092)

- Decides and fixes the D-090 audit findings: facts every consumer must agree on now come from Rust instead of being mirrored or derived in TypeScript.
- Group A (`66f1dde`): engine, settings and history facts generated as constants (SAMPLING_PLANS, METRIC_CODES, tiers), process refusal from Rust. Verified: `make check` (Vitest 758), `perf_gates` trayOnly 8.47/7.74 allocs/tick.
- Group B (`36dd642`): net/disk totals as catalog metrics (a gap when any part is), HostInfo `gpu_dvfs_mhz` and `boot_mounts`, LiveStatus `primary_iface` and `power_source`, `self.cpu` as Mean, memory composition over `mem_total_bytes`, Overview disk from `disk.used`, mock held staleness. Verified: Vitest 761, e2e 214 passed / 8 skipped, `perf_gates` trayOnly 8.34/7.73 allocs/tick, store payload 876,264 B/h (gate 1.2 MB).
- Group C (`d5801f5`): span-weighted rollups, history through now (the engine's recent bucket rows merged over the store), `HistorySeries.hold_ms` joins on the Timeline, Live re-reading when a bucket closes, local battery hours, window statistics on the `seriesWindow` grid. Verified: Vitest 760; `perf_gates` trayOnly 8.72/8.04 allocs/tick against 8.46/7.79 for the same-sitting baseline; store payload 879,144 B/h; e2e full suite 214 passed on rerun. The `dashboardOpen` long-task gate sits at the 50 ms limit under full-suite load, the same on HEAD before the change, and passed 8 of 8 in isolation.
- Real app checked: `bun run tauri dev` with the bench scenario after 7 minutes tray-only; the Power page's "Power by component" stack is continuous over the 5-minute window (2026-10-06).
- Review fixes: Rust 2d59c37 (rollup span weights come from the collector's previous read, so a slow-down or a read without a value no longer skews them; clearing history also drops the engine's recent rows; `hold_ms` takes the slowest period for a range that started before the period last got faster; recent rows kept for 15 min, exported as `HISTORY_RECENT_MS`; own-process list checked against start time; `battery_hours` reads quarter-hours past the minute tier). TS f9de5d3 (closed live buckets recomputed while a late sample can still fill them; the timeline draws a bucket only after a read made after it closed, and reads just the tail; overview network figures are totals; local battery hours across DST). Each new regression test was revert-verified. perf_gates trayOnly 8.82 / 8.07 allocs/tick (ceiling 10); Vitest 777; e2e 214 passed.
- Merged main's measured overhead and size estimates: the overhead sentence keeps main's rules (readings since the sampling setup changed, the spanning reading skipped, "Measuring…" until two follow, nothing while paused) and takes the mean with `windowMean` over the window cut to start after the spanning reading, so readings stay span-weighted. The mock's `self.cpu` period comes from the catalog (`METRIC_PERIODS_MS`, generated) instead of a 10 s literal. Verified: Rust 656 tests incl. `perf_gates` (trayOnly 8.84 / 8.07 allocs/tick), Vitest 796, e2e 217 passed / 8 skipped; `dashboardOpen` flaps at 50 to 57 ms under load on this branch, main and the merge alike.
- Left open: per-app network rows from the engine's ring survive "Clear history" for up to an hour (the series rollups no longer do); on-battery cost of the power-source notifications not measured; Timeline Live, battery bars and history-unavailable not yet checked on real hardware.
