# Decisions

This is an ADR-style log. Each entry records what was decided, why, what it costs, and the condition that should make us look at it again. Entries are appended over time and never deleted; a reversed decision gets a new entry that supersedes the old one, and the old entry's status changes to point at it.

Entries D-001 to D-014 come from the decisions confirmed with the user while writing the blueprint. D-015 is the App Store question, still open. D-016 to D-024 come from the architecture review. D-025 records the move of WidgetKit from v3 to v2. D-026 to D-029 come from tooling and the answers to v1's open questions. D-030 to D-032 come from v1.0 phase 0 setup.

See [README](README.md) for the roadmap and [architecture.md](architecture.md) for the designs these decisions refer to.

## Index

| ID | Decision | Status |
|---|---|---|
| D-001 | Desktop widgets: floating windows and WidgetKit | Accepted |
| D-002 | Start unsigned; signing and notarization later | Accepted |
| D-003 | Roadmap order | Accepted, amended by D-025 |
| D-004 | Staged v1 cut | Accepted |
| D-005 | Persistent tiered history in SQLite | Accepted |
| D-006 | Remote hosts: headless agent with own history, over SSH | Accepted |
| D-007 | Remote UI: fleet view plus host switcher | Accepted |
| D-008 | Linux extras in scope, NVIDIA out | Accepted |
| D-009 | Color follows the mocks; per-module accents | Accepted |
| D-010 | WidgetKit signing: prototype on a free Personal Team | Accepted, amended by D-025 |
| D-011 | Series data model | Accepted |
| D-012 | CBOR wire codec with capability handshake | Accepted |
| D-013 | Persisted per-machine host UUID | Accepted |
| D-014 | In-process engine, single writer per DB | Accepted |
| D-015 | App Store distribution | Open |
| D-016 | Sources produce; UI reads the local store | Accepted |
| D-017 | Per-(host, tier) cursors with a DB epoch | Accepted |
| D-018 | One table per tier, no per-day partitions | Accepted |
| D-019 | Defer the WidgetFeed sink and widget manifest until their consumers exist | Accepted, timing amended by D-025 |
| D-020 | tauri-specta for typed IPC | Accepted |
| D-021 | React Router in memory mode, routed by window label | Accepted |
| D-022 | Custom SVG for live charts, uPlot for history | Accepted |
| D-023 | Tray icons rendered with tiny-skia | Accepted |
| D-024 | Popover as a tauri-nspanel non-activating panel | Accepted |
| D-025 | WidgetKit dev build in v2 via a self-signed identity and a file feed; public distribution in v3 | Accepted |
| D-026 | Biome instead of ESLint + Prettier | Accepted |
| D-027 | Rename the app from Vitals to Kelvo | Accepted |
| D-028 | Support the latest two macOS major versions | Accepted |
| D-029 | Quit and Force Quit on the Processes page in v1, with guardrails | Accepted |
| D-030 | Tauri 2.12.1; atomic tray icon + template setter | Accepted |
| D-031 | tauri-specta 2.0.0-rc.25, exact pins | Accepted |
| D-032 | CI on hosted runners: macos-26 plus the xcode-27 preview image | Accepted |
| D-033 | Popover panel via tauri-nspanel 2.1.0 works; hide on resign-key, not the panel delegate | Accepted |
| D-034 | Native vibrancy without the macOSPrivateApi flag | Accepted |
| D-035 | WebContent termination via Tauri's on_web_content_process_terminate | Accepted |
| D-036 | Occlusion via NSWindowDidChangeOcclusionStateNotification from objc2 | Accepted |
| D-037 | Status item order: NSStatusItem reachable without a fork; reorder persistence unverified | Accepted |
| D-038 | `#[serde(other)]` does not skip unknown messages with content; decode peeks the tag | Accepted |
| D-039 | Serialized names: snake_case enum values, `JsSafeInt` for i64 over IPC | Accepted, open item settled by D-040 |
| D-040 | Wire-facing and stored enums get an `Unknown` fallback | Accepted |
| D-041 | kelvo-store details the architecture DDL left open: cursors, dedupe keys, process rows | Accepted |
| D-042 | Vendor macmon's IOReport/SMC/HID code; SMC sensor keys from Stats; GPU split from IORegistry | Accepted |
| D-043 | PMP energy counters refresh every ~5 minutes on macOS 27; PMP-derived power is a gap until they move | Accepted |
| D-044 | Collector trait shape | Accepted |
| D-045 | Public-API collector sources and process visibility without root | Accepted |
| D-046 | Engine input queue, Ticker and PowerSignals shape | Accepted |
| D-047 | Frames carry raw and held values; the catalog carries a nominal cadence | Accepted |
| D-048 | Bindings export in unified mode, not serde phases | Accepted |
| D-049 | Live channel protocol: frames carry raw and held values, resume backfills only the missed span | Accepted |
| D-050 | Settings owner shape | Accepted |
| D-051 | Shell housekeeping: host id, logs, login item, pruning | Accepted |
| D-052 | Light muted-foreground is #63636b, not zinc-500 | Accepted |
| D-053 | Frontend live state and dev entry points | Accepted |
| D-054 | On the M3 Max, CPU power comes from the SMC P-cluster keys, calibrated live to PMP | Accepted, amended by D-065 |
| D-055 | Temperatures are sampled every 5 ticks | Accepted |
| D-056 | Tray, popover panel and window lifecycle details | Accepted |
| D-057 | History stays under a byte cap and stops growing on a nearly full disk | Accepted, amended by D-059 and D-076 |
| D-058 | Kelvo has zero runtime dependencies to install | Accepted |
| D-059 | History size limit is a setting; the store's health reaches the UI | Accepted |
| D-060 | Frontend performance gate in Playwright, thresholds in `perf-budget.json` | Accepted |
| D-061 | Sampling intervals from 0.5 s to 60 s; collector cadences are wall-clock periods; tray-only IOReport at 10 s | Accepted |
| D-062 | Engine performance gates: allocations, OS calls, store volume, CPU | Accepted |
| D-063 | Live ring as typed series columns; streaming charts still rebuild their path each tick | Accepted |
| D-064 | Store and engine hardening from the architecture review: one writer, clock steps, peer identity, gated row kinds, typed store errors | Accepted, amended by D-070 |
| D-065 | Quit's OS side moves to kelvo-collect; the appstore feature reaches the app; CPU power calibration persists per chip | Accepted |
| D-066 | The live channel per host: a hub that owns the ring, projected channels, paced process rows, chunked backfill | Accepted |
| D-067 | Idle CPU is measured end to end; collectors not on screen sample every 10 s; CPU gates get a fixed target and a baseline ratchet | Accepted, amended by D-070 |
| D-068, D-069 | Not used (numbers skipped) | |
| D-070 | Gauges sample every tick; a backward clock step holds history for at most an hour; lost gap opens are recovered; perf and prune tests measure what they claim | Accepted |
| D-071 | The host id is bound to the Mac; unusable data and log directories never stop the launch | Accepted |
| D-072 | The live channel's clock steps are an explicit timeline; resumes go in chunks; display sleep and frame pacing reach the status | Accepted |
| D-073 | The tray swaps its image straight into a pinned-length status item | Accepted |
| D-074 | The data directory is owner-only | Accepted |
| D-075 | `make perf` runs one engine at a time with a 30% band; process-row allocations no longer follow process churn; dependency audit in CI | Accepted |
| D-076 | History older than 7 days is kept in 15-minute buckets | Accepted |
| D-077 | The tray redraws at most every 2 s while sampling stays at 1 s | Accepted |
| D-078 | Release infrastructure moves after v1.2 and manual QA; performance is accepted as it stands | Accepted |
| D-079 | Long-range reads report their slot width; the heatmap takes local hours from the frontend; CSV export streams from the store | Accepted |
| D-080 | Own-item menu bar modes: one status item per module, graphs from board 01, history in the tray | Accepted |
| D-081 | Per-process network uses NetworkStatistics in process; it sees only the current user's flows | Accepted, amended by D-089 |
| D-082 | Per-process network: describe flows to learn their owner, rows only for the user's processes, no remainder row | Accepted, amended by D-089 |
| D-083 | Event detectors run on the persisted tick; events flush on write and reach windows as a pushed event | Accepted |
| D-084 | Two built-in alert rules, off by default, posted through tauri-plugin-notification; a click cannot open the Timeline | Accepted |
| D-085 | Per-process GPU reads the GPU's IORegistry user clients; shares of the whole GPU, clamped, approximate under long command buffers | Accepted |
| D-086 | One-shot motion choreography is allowed; motion tokens split by cost, and power saver stops only per-tick tweens | Accepted |
| D-087 | Headline numbers count to their next value on large moves; parsed from the formatted text, log space on unit ladders, off in power saver | Accepted |
| D-089 | Per-process network history: always-on per-app bytes, persisted, with a remainder split | Accepted |
| D-090 | The live stream publishes each series' kind and how long a sample stays current; charts join and fill samples from those facts | Accepted |
| D-091 | One chart window for every module page, saved as a setting | Accepted |
| D-092 | Rust publishes the remaining data facts: engine constants, process refusal, totals, host facts and span-weighted history | Accepted |
| D-093 | Energy by app over the chart window from an in-memory ring; local and public IP on Network; brush selections dismiss like a d3 brush; °F by default | Accepted |
| D-095 | Public release prep: Apache-2.0, bundle identifier com.tryopendata.kelvo, design mocks retired | Accepted |
| D-096 | macOS checks move to a pre-push hook; hosted CI runs Linux jobs on push | Accepted |

---

## D-001: Desktop widgets: floating windows and WidgetKit

Status: Accepted. Date: 2026-10-05.

### Context

The mocks show live desktop widgets in an edit mode (board 10) and a composer (board 09). macOS also has a native widget system, WidgetKit, which appears in the widget gallery and Notification Center. The two serve different needs: floating windows can update every second and look exactly like the popover cards, while WidgetKit widgets are system-managed, refresh on a budget, and live where users expect widgets to live.

### Decision

Build both. Floating desktop windows come first because they reuse the React widget components directly. WidgetKit follows, with widget kinds generated from the same manifest.

### Consequences

Two render paths for widgets: React for floating windows and the popover, SwiftUI for WidgetKit. The widget manifest and the render-only boundary rule exist to keep their data contracts identical. Floating windows add a WebContent process per display, which needs its own budget.

### Revisit when

WidgetKit refresh limits make its widgets visibly stale for metrics people care about, or floating windows turn out to cost more than their per-display budget.

---

## D-002: Start unsigned; signing and notarization later

Status: Accepted. Date: 2026-10-05.

### Context

Developer ID signing and notarization need a paid Apple Developer membership and CI secrets. None of the v1 features need them, and setting them up early slows the first release.

### Decision

Ship v1 as an ad-hoc signed DMG on GitHub Releases plus a personal Homebrew tap, with documented first-launch steps. The Tauri updater uses its own minisign key. Developer ID, notarization, the official cask and App Store evaluation come in v3, but the infrastructure they need (entitlement declarations, an `appstore` feature seam) is in place from v1.

### Consequences

First launch shows a Gatekeeper warning and users must approve it once. Some users will not install an unsigned app. Whether files downloaded by the updater get the quarantine flag is unverified; if they do, each update repeats the first-launch dance.

### Revisit when

The quarantine behaviour of updater downloads is confirmed to be bad, or adoption data shows the Gatekeeper step is losing a meaningful share of installs.

---

## D-003: Roadmap order

Status: Accepted, amended by D-025. Date: 2026-10-05.

### Context

There are four large bodies of work: the local monitor, customization and widgets, native distribution, and remote hosts. They depend on each other unevenly. Remote hosts are the most speculative and the most expensive.

### Decision

The original order was v1 local monitor, v2 customization and desktop widgets, v3 native distribution and WidgetKit, v4 remote hosts. D-025 moves the WidgetKit dev build into v2.2, leaving v3 as native distribution (including public WidgetKit distribution and App Store evaluation). v4 stays remote hosts.

### Consequences

The local product is complete and useful before any remote work starts. v1 has to lay infrastructure for v4 without building it, which is what the "Infrastructure laid in v1" section of the architecture doc is for.

### Revisit when

Users ask for remote monitoring more than for customization, or a v2 feature turns out to depend on a distribution capability that only v3 provides.

---

## D-004: Staged v1 cut

Status: Accepted. Date: 2026-10-05.

### Context

The 15 mock boards describe a lot of product. Shipping all of it as v1.0 would delay the first installable build by months.

### Decision

The mocks are the target for v1.x and v2.x, not all of v1.0. v1.0 ships the tray, popover, dashboard with Overview and module pages, Timeline at 1h and 24h, Settings, onboarding and the empty/gap/unsupported states. v1.1 adds 7d/30d, the heatmap, CSV export and more tray styles. v1.2 adds per-process network and GPU, auto-annotations and the first alert.

### Consequences

Each minor release must be installable on its own. Some mocked screens (the 30-day heatmap, graph tray styles) appear later than their mock suggests.

### Revisit when

A minor release grows past the point where it can ship in a few weeks.

---

## D-005: Persistent tiered history in SQLite

Status: Accepted. Date: 2026-10-05.

### Context

History with attribution is the main differentiator. "What made my fans spin up at 3pm" needs data from 3pm. Competitors keep little or no history.

### Decision

Keep history in SQLite with tiers: a 1-hour in-memory ring at 1s, 10s buckets for 24 hours, 1-minute buckets for 30 days (configurable), plus process snapshots and explicit gap and event rows.

### Consequences

A disk budget (150 MB for 30 days on one host) that has to be enforced and tested. Pruning and vacuum logic. A write path that must stay cheap enough not to show up in the CPU budget.

### Revisit when

The synthetic fill test cannot meet the budget on wide machines, or query latency on 30 days becomes noticeable.

---

## D-006: Remote hosts: headless agent with own history, over SSH

Status: Accepted. Date: 2026-10-05.

### Context

Developers want to watch Linux servers and other Macs. A remote host might be unreachable for hours, and history from that period should not be lost.

### Decision

A headless `kelvo-agent` binary built from the same engine runs on each remote host, keeps its own tiered history, and is reached with `ssh host kelvo-agent serve --stdio`. The controller streams live data and syncs closed buckets with cursors. Install and update happen over SSH using `~/.ssh/config` and agent auth.

### Consequences

No new network service or port to secure; SSH handles auth and transport. The agent must stay small (under 0.3% CPU and 30 MB). The protocol must survive version skew between controller and agent.

### Revisit when

Users need to monitor hosts they cannot SSH into, or the SSH session overhead shows up in the agent's budget.

---

## D-007: Remote UI: fleet view plus host switcher

Status: Accepted. Date: 2026-10-05.

### Context

With several hosts, users need an overview and a way to look at one host in detail.

### Decision

A Hosts fleet view with a card per host (sparklines, online state), and a host switcher that re-scopes every existing view to the selected host.

### Consequences

Every view must be host-scoped from v1 (D-013). The fleet view must not require a full live stream per host, so a `HostSummary` message type is reserved in the protocol from v1.

### Revisit when

Users regularly compare hosts side by side, which the switcher does not support.

---

## D-008: Linux extras in scope, NVIDIA out

Status: Accepted. Date: 2026-10-05.

### Context

Linux servers have things Macs do not: cgroups, containers, systemd units. They also often have NVIDIA GPUs, which need NVML and a driver-specific dependency.

### Decision

The Linux collector set covers `/proc`, sysfs, hwmon, thermal zones, cgroups v2 and containers (Docker, Podman, systemd units), plus remote alerts and remote metrics in the menu bar. NVIDIA GPU support is out.

### Consequences

Container series have capped cardinality (enforced by the catalog). Users with NVIDIA servers will see no GPU module on those hosts.

### Revisit when

A contributor offers to maintain an NVML collector, or NVIDIA hosts turn out to be a large share of v4 users.

---

## D-009: Color follows the mocks; per-module accents

Status: Accepted. Date: 2026-10-05.

### Context

opendata's design spec has a single brand accent. The mocks instead give each module its own accent, and use cyan both for CPU and for calls to action.

### Decision

Follow the mocks. CPU cyan (also the CTA color), GPU pink, Memory violet, Power & Sensors amber, Network emerald, Disk blue, Battery lime. Series inside a card use a lightness ramp of the card's accent.

### Consequences

Warning states (amber, red) can be confused with the amber Power accent, so warnings always carry an icon and text, never color alone. Cyan CTAs sit next to cyan CPU charts; the design system doc defines how they stay distinguishable.

### Revisit when

Accessibility testing shows module accents or warnings are hard to tell apart.

---

## D-010: WidgetKit signing: prototype on a free Personal Team

Status: Accepted, amended by D-025. Date: 2026-10-05.

### Context

WidgetKit extensions need code signing. Public distribution needs the paid Apple Developer Program. Waiting for membership to start widget work would block it on a purchase.

### Decision

Prototype WidgetKit locally with a free Personal Team. Gate only distribution on paid membership. The `native/KelvoWidgets` project takes its team ID and group identifiers from an `.xcconfig` so switching to a paid team is a config change. D-025 refines how a dev build signs and shares data without a team.

### Consequences

Personal Team provisioning is reportedly finicky (unverified). Dev builds work on the developer's own Mac only.

### Revisit when

Paid membership is in place (v3).

---

## D-011: Series data model

Status: Accepted. One-way door. Date: 2026-10-05.

### Context

The set of measured things varies by chip, changes at runtime (disks, interfaces, eGPUs), and on Linux includes containers that come and go. A typed struct per sample breaks on all of these and would need a migration per hardware variant. The architecture review flagged this as the decision that is hardest to change after data exists on disk and on the wire.

### Decision

Store and sync series: a `metric_id` plus labels, interned per host, with rows referencing immutable layouts. The typed `Snapshot` in `kelvo-schema` is a view built from series for the UI and tray. It is never stored or synced.

### Consequences

Storage rows are opaque blobs that need a layout to decode. Queries go through the store crate, not ad-hoc SQL. Adding a metric is a catalog entry, not a migration. Container cardinality must be capped. The UI gets `None` for missing series, which is how the never-interpolate rule reaches charts.

### Revisit when

Practically never; a change here means migrating every user's history. A new requirement that cannot be expressed as a series would be the trigger.

---

## D-012: CBOR wire codec with capability handshake

Status: Accepted. One-way door. Date: 2026-10-05.

### Context

The controller and remote agents will run different versions. The protocol carries numeric-heavy frames every second over SSH.

### Decision

Length-prefixed CBOR frames via `ciborium`, with a handshake that exchanges protocol version, host identity, DB epoch and capabilities. Unknown `metric_id`s and unknown message types are ignored. A codec round-trip and skew test runs in CI from v1.

### Consequences

The webview path (JSON via tauri-specta) does not exercise the codec, so the CI skew test is the only thing protecting it until v4. Whether `#[serde(other)]` skips unknown content-carrying variants is unverified and is the first thing that test checks.

### Revisit when

Profiling shows CBOR encode/decode in the agent's CPU budget, or a peer outside Rust needs to speak the protocol.

---

## D-013: Persisted per-machine host UUID

Status: Accepted. One-way door. Date: 2026-10-05.

### Context

v4 adds remote hosts. If v1 treats "local" as the identity, every command, query key and store needs a new parameter later.

### Decision

Each machine gets a UUID on first run. `host_id` is that UUID everywhere outside SQLite. "Local" is a flag on the host record. The UI is keyed `hosts[hostId]` from v1.

### Consequences

Slight overhead in v1, where there is one host. Inside SQLite the UUID is interned to an integer that never leaves the DB. Reinstalling the app produces a new UUID unless the data directory survives; v4 handles that with the DB epoch (D-017).

### Revisit when

Users want to rename or merge hosts across reinstalls.

---

## D-014: In-process engine, single writer per DB

Status: Accepted. Date: 2026-10-05.

### Context

A separate launchd daemon would keep collecting while the app is closed, at the cost of a second process, IPC, install complexity and a harder story for signing. Two processes writing one SQLite file is a known source of locking bugs.

### Decision

The engine runs inside the app process in v1. Exactly one process, and one thread within it, writes a given SQLite file. Settings and layouts are also single-writer: Rust owns them, windows go through commands, and a `settings-changed` event updates their mirrors.

### Consequences

When the app is not running, nothing is collected; the gap is recorded as `app_not_running`. Launch at login covers most of this. WidgetKit widgets (v2.2) go stale while the app is closed.

### Revisit when

WidgetKit lands in v2.2 and stale widgets with the app closed become a real complaint, or the v4 agent design suggests running the local engine as an agent on a unix socket. `LocalSource` is the seam that makes that switch invisible to the UI.

---

## D-015: App Store distribution

Status: Open. Date: 2026-10-05.

### Context

The App Store requires the sandbox. The sandbox blocks SMC, IOReport and private IOKit/NetworkStatistics access, so a store build would lose Power & Sensors and per-process GPU and network data, which are among the main differentiators.

### Decision

Not decided. v1 makes the evaluation cheap: each collector declares the entitlements it needs, and an `appstore` Cargo feature drops collectors that a sandboxed build cannot hold, with the UI showing "not available in this edition" through the normal capabilities path. v3 builds that edition, lists which modules survive, and records a go/no-go here.

### Consequences

Until decided, no feature may depend on being outside the sandbox in a way that bypasses the collector entitlement declarations.

### Revisit when

v3 evaluation.

---

## D-016: Sources produce; UI reads the local store

Status: Accepted. Date: 2026-10-05.

### Context

An earlier sketch had the UI query a `Source` for both live and historical data. For a remote host that makes every Timeline scroll and every fleet card an SSH round trip, and an offline host shows nothing.

### Decision

The `Source` trait is `start(sink) / host / capabilities`. Sources push live frames to the bus and persisted rows into the controller's store. Every UI query goes to the controller by `host_id`. In v1, `LocalSource` wraps the in-process engine; in v4, `RemoteSource` speaks `kelvo-proto`.

### Consequences

The controller keeps a mirror of each remote host's history, which needs a per-host disk budget in v4. The UI has one code path for local and remote hosts.

### Revisit when

Mirror size becomes a problem for users with many hosts and on-demand remote queries are needed for long ranges.

---

## D-017: Per-(host, tier) cursors with a DB epoch

Status: Accepted. Date: 2026-10-05.

### Context

Syncing history from an agent that may have been offline, reinstalled or pruned needs a resumable position that detects all three.

### Decision

Every persisted row has a `seq` monotonic per DB. Cursors are `(db_instance_uuid, seq)`, kept per `(host, tier)`. An epoch mismatch triggers a full resync. Pruned-past-cursor replies `Truncated{earliest}` and the controller writes a gap. Ingest upserts on `(host, bucket_ts, layout_id)` so replays are harmless. Only closed buckets are synced, and the controller never recomputes rollups.

### Consequences

v1 must write `seq` and create `db_instance_uuid` even though nothing reads them until v4. Adding `seq` later to populated tables would have no correct ordering.

### Revisit when

Sync needs to go both ways (controller to agent), which this design does not cover.

---

## D-018: One table per tier, no per-day partitions

Status: Accepted. Date: 2026-10-05.

### Context

Per-day tables make pruning a cheap `DROP TABLE`, but range queries must union across tables and cursors must span table boundaries.

### Decision

One table per tier, `(host_id, bucket_ts, layout_id, seq, blob)` with a unique index on the key and an index on `seq`. Pruning is a batched delete plus incremental vacuum.

### Consequences

Pruning costs more than dropping a table, but at this data size (tens of MB per tier) a batched delete every few minutes is cheap. Queries and cursors stay simple.

### Revisit when

Pruning shows up in the CPU budget, or v4 mirrors push a single tier table past a few hundred MB.

---

## D-019: Defer the WidgetFeed sink and widget manifest until their consumers exist

Status: Accepted, timing amended by D-025. Date: 2026-10-05.

### Context

The WidgetKit feed and the widget manifest are easy to design badly before there is a widget composer or a Swift extension to consume them.

### Decision

Do not build either in v1. The widget manifest arrives with the composer in v2.0. The WidgetFeed sink arrives as a bus subscriber in v2.2 (originally v3; moved by D-025). v1 only guarantees that widget components are render-only, so their props are the data contract the manifest will describe.

### Consequences

v1 widget-like components (popover cards, Overview cards) are written against props, not stores, which the Biome boundary rule enforces.

### Revisit when

v2.0 design starts.

---

## D-020: tauri-specta for typed IPC

Status: Accepted. Date: 2026-10-05.

### Context

The frontend calls many commands and listens to events whose payloads are `kelvo-schema` types. Hand-written TS types drift. opendata solves the same problem for HTTP with Orval.

### Decision

Generate TS bindings for every command, event and schema type with `tauri-specta` into `src/core/generated/`. CI fails if the checked-in bindings differ from the generated ones. `i64` values are exported as `number`, with the 2^53 bound documented.

### Consequences

tauri-specta for Tauri 2 may still be a release candidate (unverified). The version is pinned and checked in phase 0. If it is unusable, the fallback is `specta` alone with a small hand-rolled command wrapper.

### Revisit when

tauri-specta stalls or breaks on a Tauri upgrade.

---

## D-021: React Router in memory mode, routed by window label

Status: Accepted. Date: 2026-10-05.

### Context

Each Tauri window loads the same frontend bundle. There is no URL bar and no SSR.

### Decision

React Router in data/library mode with memory history. `main.tsx` reads the window label and picks the initial route: `popover`, `dashboard/*`, `onboarding`, `board/:display`.

### Consequences

No deep links from outside the app without a command. opendata's loader, SSR and `.server` patterns do not apply.

### Revisit when

A feature needs real URLs, for example `kelvo://` deep links from notifications.

---

## D-022: Custom SVG for live charts, uPlot for history

Status: Accepted. Date: 2026-10-05.

### Context

Live charts redraw every second on small surfaces and must match the mocks exactly. History charts draw tens of thousands of points with a synced crosshair across lanes.

### Decision

Live charts are custom SVG on `d3-scale` and `d3-shape`, scrolling with `translateX`, with tweens off in Low Power Mode and on battery. History uses `uPlot` on canvas with a synced cursor. The 30-day heatmap is a plain canvas.

### Consequences

Two chart stacks to style consistently; the design system defines shared tokens for both. uPlot's styling is less flexible than SVG, so Timeline visuals may need small compromises against the mock.

### Revisit when

uPlot cannot render a mocked Timeline detail (sleep bands, annotation markers) without heavy patching.

---

## D-023: Tray icons rendered with tiny-skia

Status: Accepted. Date: 2026-10-05.

### Context

The combined tray icon draws bars and a temperature value in a 17×14 pt template image, redrawn every second. Rendering through the webview would wake WebKit every second. Using AppKit drawing directly ties the code to Objective-C.

### Decision

Render tray images in Rust with `tiny-skia` and `ab_glyph`, quantize values to the pixels the icon can show, hash the frame, and skip unchanged frames. Set image and template flag atomically (Tauri 2.12 or later, unverified).

### Consequences

A bundled font for icon text. The WindowServer cost is controlled by the skip logic, measured against a 0.2% budget.

### Revisit when

Template rendering does not match native status items closely enough, or WindowServer cost exceeds budget.

---

## D-024: Popover as a tauri-nspanel non-activating panel

Status: Accepted. Date: 2026-10-05.

### Context

A normal Tauri window steals focus when shown, which feels wrong for a menu bar popover, and creating it on each click is slow.

### Decision

Use a `tauri-nspanel` non-activating `NSPanel`, created hidden at startup and kept warm. Rust starts and stops its live channel based on visibility and occlusion, sends a ring-buffer backfill on show, and reloads it if the WebContent process terminates.

### Consequences

The warm panel's memory counts against the 150 MB coalition budget. A third-party plugin is in the critical path of the main UI surface.

### Revisit when

tauri-nspanel falls behind Tauri releases, or the warm panel cannot fit the memory budget.

---

## D-025: WidgetKit dev build in v2 via self-signed identity and a file feed; public distribution in v3

Status: Accepted. Date: 2026-10-05. Amends D-003, D-010 and D-019.

### Context

The original plan put WidgetKit in v3 because distribution needs paid membership. Research shows a free Apple account can run WidgetKit widgets on the developer's own Mac, which makes a personal-use dev build possible much earlier. The approach is unverified end to end. Three constraints shape it. WidgetKit caches the extension by signing identity, so ad-hoc signing (whose cdhash changes every build) drops the widget from the gallery after rebuilds. The extension must be sandboxed. App Groups need a team, so a dev build cannot use one.

### Decision

Ship a WidgetKit dev build ("dev build, personal use") in v2.2, after the widget manifest (v2.0) and floating desktop widgets (v2.1).

The app and the extension are signed with a stable local identity, either a self-signed code-signing certificate or the Personal Team "Apple Development" certificate. The unsandboxed Tauri app writes JSON into `~/Library/Application Support/Kelvo/widget-feed/`, and the sandboxed extension reads it through a read-only, home-relative temporary-exception entitlement. Widget kinds come from the v2 manifest. The feed is written by a `WidgetFeedSink` bus subscriber, with a file implementation now and an App Group implementation in v3.

Bundling builds the Xcode widget target via `beforeBundleCommand` (`xcodebuild`), signs the `.appex` first, embeds it via `bundle.macOS.files` at `PlugIns/KelvoWidgets.appex`, and signs the outer app with the same identity. `CFBundleVersion` is bumped every build to avoid stale chronod renders. `WidgetCenter.reloadAllTimelines` needs a small Swift bridge; otherwise the extension relies on its timeline policy. The app must launch once before widgets appear in the gallery.

v3 handles public WidgetKit distribution: Developer ID, notarization, and a switch to a Team-ID-prefixed App Group, which also avoids the macOS 15 "access data from other apps" prompt.

### Consequences

The new roadmap is v2.0 manifest and popover composer, v2.1 floating desktop widgets, v2.2 WidgetKit dev build, v2.3 alert editor, configurable Overview and per-widget history. v3 becomes native distribution only. The `WidgetFeedSink` trait must hide the file-versus-App-Group difference so v3 is a swap of implementation. Temporary-exception entitlements are not acceptable for App Store review, which matters if D-015 goes toward the store. WidgetKit widgets go stale when the app is closed, which brings the launchd helper question in D-014 forward.

### Revisit when

The dev-build approach fails end to end on current macOS (signing, entitlement or gallery caching behaves differently than researched), or paid membership arrives early enough to skip the file feed.

---

## D-026: Biome instead of ESLint + Prettier

Status: Accepted. Date: 2026-10-05.

### Context

The frontend tooling was first ported from opendata, which uses an ESLint flat config (typescript-eslint, react, react-hooks, jsx-a11y) plus Prettier with `prettier-plugin-tailwindcss`. The user prefers Biome for Kelvo.

### Decision

Use Biome 2.x (`biome.json`) as the single formatter and linter. Formatting matches the old Prettier settings: double quotes, semicolons, es5 trailing commas, 2-space indent. The linter runs the recommended preset plus the whole a11y group, `useExhaustiveDependencies`, `useHookAtTopLevel`, and nursery `useSortedClasses` for Tailwind class order (also applied inside `cn()`, `cva()` and `clsx()`). The architectural boundaries (no React in `src/core/`, render-only `src/app/widgets/`, no direct `@tauri-apps/*` in `src/app/`) are `noRestrictedImports` rules in path-scoped `overrides`. A PreToolUse hook (`enforce-boundaries.sh`) also enforces them when an agent writes a file.

### Consequences

One tool, one config and a much faster lint/format pass in hooks and pre-commit. The project diverges from opendata's tooling, so lint config can't be copied between the repos. Biome's a11y rules are shallower than `eslint-plugin-jsx-a11y`. `useSortedClasses` is a nursery rule and doesn't match `prettier-plugin-tailwindcss` exactly: it doesn't read the Tailwind config, and its fix can differ on custom utilities and variants. Biome's React hooks rules are close to `eslint-plugin-react-hooks` but don't include the React Compiler rules.

### Revisit when

A needed lint has no Biome equivalent, such as a type-aware rule or React Compiler diagnostics, or `useSortedClasses` produces churn against Tailwind v4 custom utilities.

---

## D-027: Rename the app from Vitals to Kelvo

Status: Accepted. Date: 2026-10-05.

### Context

The working name was Vitals. The planned install path includes a Homebrew cask, and the cask name `vitals` is already taken by hmarr/vitals, an unrelated macOS system monitor. Shipping under the same name as an existing monitor would confuse search, the tap, and users. `kelvo` is free as a cask name.

### Decision

The app is Kelvo: product name "Kelvo", bundle identifier `com.riley.kelvo`, crates `kelvo-*`, agent binary `kelvo-agent`, cask `kelvo`. All plan docs, configs and paths use the new name.

### Consequences

The rename happens before any release, so there is nothing to migrate. No trademark search has been done yet; a conflict found later would mean a second rename, which gets more expensive after v1.0 ships with a bundle identifier and an updater feed.

### Revisit when

A trademark search turns up a conflict, which should be checked before the v1.0 release.

---

## D-028: Support the latest two macOS major versions

Status: Accepted. Date: 2026-10-05. Resolves v1 open question Q4.

### Context

Kelvo reads private interfaces (IOReport, SMC, HID sensors) whose behavior changes between macOS releases, and the vendored code is only as good as the versions it is tested on. Testing many old releases is not realistic for one developer, and the people Kelvo is for update macOS promptly.

### Decision

Kelvo supports the current macOS major version and the one before it (N-1). Today that is macOS 27 and macOS 26, so `minimumSystemVersion` is "26.0". When a new major version ships, the floor moves up in the next minor release, and the oldest version drops out. Both supported majors are tested before each release: the accuracy script, the packaged-app checklist, and the private-interface collectors.

### Consequences

Code can use APIs available on macOS 26 without fallbacks. Users on older versions stay on the last release that supported them; the updater must not offer them a build they cannot run (check that Tauri's updater respects `minimumSystemVersion`, unverified). Testing two majors needs either two Macs, a second boot volume, or a VM for the older one.

### Revisit when

Users on N-2 ask in numbers, or a private interface breaks in a way that makes N-1 support expensive.

---

## D-029: Quit and Force Quit on the Processes page in v1, with guardrails

Status: Accepted. Date: 2026-10-05. Resolves v1 open question Q1, which proposed leaving this out.

### Context

The Processes page shows every process. The plan first excluded acting on them, because a monitor that can kill processes can lose user data, and Activity Monitor already does it. The user wants it anyway: finding a runaway process and then switching to Activity Monitor to stop it is the friction Kelvo is supposed to remove.

### Decision

The Processes page gets Quit and Force Quit as a row action and in the row context menu. Quit sends a polite termination (`NSRunningApplication.terminate` for apps, `SIGTERM` otherwise); Force Quit sends `forceTerminate` or `SIGKILL`. Both ask for confirmation, and the Force Quit dialog warns that unsaved data is lost. The Rust command `process_signal(host, pid, start_time, kind)` checks that the PID still belongs to the process the user picked (matching start time) before signalling, refuses PID 1, `kernel_task`, `WindowServer` and Kelvo itself, and returns a typed error on `EPERM` for processes owned by another user, which the UI shows as a toast. There is no privilege escalation.

### Consequences

Kelvo can now cause data loss, so the confirm dialogs and the refuse list are part of the feature, not polish. The command is local-only in v1; v4 remote hosts would need a separate decision before signals cross the wire. An App Store edition would lose the ability to signal other apps' processes outside its sandbox (unverified how much remains).

### Revisit when

Users ask to signal processes owned by other users, or v4 wants the same action on remote hosts.

---

## D-030: Tauri 2.12.1, with an atomic tray icon + template setter

Status: Accepted. Date: 2026-10-05. Closes the phase 0 Tauri version item.

### Context

Phase 0 asks for the latest Tauri 2.x at or above 2.12, and for confirmation that the tray icon and its template flag can be set in one call. The tray redraws its icon at 1 Hz; setting the image and then the template flag separately renders the icon twice, which shows as a flicker in the menu bar.

### Decision

Tauri 2.12.1 (`tauri` 2.12.1, `tauri-build` 2.7.1, `tauri-plugin-opener` 2.7.0; `@tauri-apps/api` and `@tauri-apps/cli` 2.12.1, `@tauri-apps/plugin-opener` 2.7.0). These are the latest 2.x on crates.io and npm as of today; the next line is 3.0.0-alpha, which we do not take. The tray uses `TrayIcon::set_icon_with_as_template(icon, is_template)`, which the 2.12.1 source documents as setting both atomically on macOS to avoid the double render (it falls back to `set_icon` elsewhere). The JS API has the same as `TrayIcon.setIconWithAsTemplate`. This was confirmed by reading the 2.12.1 crate source and the published `@tauri-apps/api` typings, not on a running tray yet; the tray work in v1.0 verifies it visually.

### Consequences

The tray renderer calls the one combined setter per tick and never the two separate ones. Tauri pulls `tray-icon` 0.25.x; `tray-icon` 0.26 is out but Tauri 2.12.1 does not use it yet, so there is nothing to do there.

### Revisit when

A Tauri 2.x release fixes something the tray or popover needs, or Tauri 3 leaves alpha.

---

## D-031: tauri-specta 2.0.0-rc.25, pinned exactly

Status: Accepted. Date: 2026-10-05. Addresses risk R4.

### Context

Rust types are the single source of truth for IPC, and the TS bindings are generated by tauri-specta. Its stable 1.x line targets Tauri 1, so Tauri 2 needs the 2.x line, which is still a release candidate. Its own docs ask users to pin with `=` during the RC period.

### Decision

`tauri-specta = "=2.0.0-rc.25"` (feature `typescript`), `specta = "=2.0.0-rc.25"`, `specta-typescript = "=0.0.12"`, all release candidates or pre-1.0, pinned exactly in `[workspace.dependencies]`. tauri-specta itself pins `specta` with `=` too, so the three move together. The app shell builds one `tauri_specta::Builder`, used both for the invoke handler and for the export, so registered commands and generated bindings cannot drift. Bindings are written to `src/core/generated/bindings.ts` on every debug launch and by `make bindings`, which runs the `export_bindings` example in `src-tauri/examples/` without starting the app. CI runs `make bindings` and fails if the directory differs from the commit.

### Consequences

Upgrading is a deliberate change: bump all three pins together, regenerate, and expect diffs in the generated file and possibly API changes in the builder. Until the first specta command lands, the generated file is just the header.

### Revisit when

tauri-specta 2.0 ships stable, or an RC bump fixes something we need.

---

## D-032: CI on hosted runners, macos-26 plus the xcode-27 preview image

Status: Accepted. Date: 2026-10-05. Closes the phase 0 CI runner items.

### Context

D-028 says both supported majors (macOS 26 and 27) are tested. Kelvo is a public repo, so GitHub-hosted runners cost nothing, while a self-hosted Mac costs upkeep and is a security risk for a public repo that accepts pull requests. GitHub's runner-images list today has GA `macos-26` (arm64, also `macos-latest`) and no `macos-27` label. The only macOS 27 image is the `xcode-27` preview label (arm64), whose base OS moved to macOS 27 on 2026-09-16 (actions/runner-images#14404, #14680). Preview images can queue and change underneath you.

### Decision

Hosted runners. The macOS job runs as a matrix on `macos-26` and `xcode-27`: fmt, clippy `-D warnings`, `cargo test --workspace`, the bindings drift check, `bun run check`, Playwright (Chromium and WebKit) and `bun tauri build --debug`. The `xcode-27` leg is `continue-on-error`, so preview flakiness does not block merges; its result still shows on the run. A Linux job on `ubuntu-latest` runs `cargo check` and `cargo test` for the five portable crates natively (not `src-tauri`).

### Consequences

macOS 27 coverage is advisory until GitHub ships a GA `macos-27` label. Before each release, a red or skipped `xcode-27` leg has to be looked at by hand, and the packaged-app checks on macOS 27 run on a real machine anyway (D-028). Playwright browsers are downloaded on every run; cache them if it becomes slow.

### Revisit when

GitHub ships `macos-27` as GA: switch the matrix to `macos-26` and `macos-27`, and drop `continue-on-error`. When macOS 28 ships, the floor moves per D-028 and the matrix follows.

---

## D-033: Popover panel via tauri-nspanel 2.1.0 works; hide on resign-key, not the panel delegate

Status: Accepted. Date: 2026-10-05. Phase 0 spike.

### Context

The popover must show under the status item without activating Kelvo or taking the menu bar from the frontmost app (D-024).

### Decision

`tauri-nspanel = "=2.1.0"` (objc2 0.6). The `popover` webview window is converted with `to_panel`, given the nonactivating style mask, status level, and `can_join_all_spaces | full_screen_auxiliary | stationary` collection behaviour, and shown with `show_and_make_key`. The spike on macOS 27.0 showed it centred under the tray rect, the frontmost app and menu bar owner unchanged across six shows, outside clicks hiding it in about 30 ms, and a second tray click closing it without a hide/reshow race. Hiding listens to Tauri's `WindowEvent::Focused(false)` (or `NSWindowDidResignKeyNotification`), never `panel.set_event_handler`, because that replaces tao's window delegate and silences every Tauri window event. The `tauri_panel!` macro goes in its own module because it injects `use` items. `NSApp.isActive()` reads true while the panel is key, so it is not a focus-theft signal; use `NSWorkspace.frontmostApplication`.

### Consequences

Open caveat, unverified: twice, the first show after launch with a fullscreen app frontmost activated Kelvo and appeared to switch Space. Phase 3 reproduces this deliberately and tries `_setPreventsActivation:` or `move_to_active_space` if it recurs.

### Revisit when

The fullscreen first-show anomaly is reproduced or ruled out in phase 3.

---

## D-034: Native vibrancy without the macOSPrivateApi flag

Status: Accepted. Date: 2026-10-05. Phase 0 spike.

### Context

The popover and v2.1 boards need a real blur material behind a transparent webview; CSS cannot blur the desktop.

### Decision

`window-vibrancy = "=0.8.1"` with `NSVisualEffectMaterial::Popover`, state Active, radius 12 on a `transparent(true)` window. Verified on macOS 27 by screenshot: blurred wallpaper behind the cards, clean rounded corners, and it survives a WebContent reload. No `macOSPrivateApi` config or `macos-private-api` feature is needed on Tauri 2.12.1, where that feature is a no-op because the APIs are always on. Tauri's `Effect::LiquidGlassRegular` also renders but left dark square artefacts at the corners, so it is not used for now.

### Consequences

wry still sets the private `drawsBackground` key for transparency, so private API use exists regardless of the flag. That matters for the v3.2 App Store evaluation. Light/dark switching of the material is not yet checked.

### Revisit when

Liquid Glass corner clipping is fixed or worked around (try `set_corner_radius` on the panel).

---

## D-035: WebContent termination via Tauri's on_web_content_process_terminate

Status: Accepted. Date: 2026-10-05. Phase 0 spike.

### Context

A popover kept warm for days will eventually lose its WebContent process; the plan reloads it the next time it is hidden.

### Decision

Use `tauri::Builder::on_web_content_process_terminate`, an app-level macOS hook that receives the `&Webview`. Verified by `kill -9` of the popover's WebContent process: the hook fired for `popover`, `reload()` brought up a new process, and the dashboard's process was untouched. No objc2 fallback is needed.

### Consequences

Registering the hook replaces Tauri's default, which reloads immediately. Kelvo's handler reloads visible windows immediately and sets a reload-on-hide flag for the popover when it is showing.

### Revisit when

Tauri changes the hook's semantics.

---

## D-036: Occlusion via NSWindowDidChangeOcclusionStateNotification from objc2

Status: Accepted. Date: 2026-10-05. Phase 0 spike.

### Context

Rust decides when a window's live channel stops (architecture item 8), and hidden WKWebView timers are not a reliable signal.

### Decision

Observe `NSWindowDidChangeOcclusionStateNotification` per window through `NSNotificationCenter` from objc2 (objc2-app-kit 0.3), reading `occlusionState().contains(Visible)`. Verified: show, hide, minimize and unminimize transitions all fire, the popover's hide registers about 270 ms after `orderOut`. It does not touch the window delegate, so it coexists with tauri-nspanel. Transparent windows do not occlude others, as expected.

### Consequences

Covered-by-another-app, display sleep and screen lock were not tested; phase 3 confirms them and keeps the display-sleep signal from PowerSignals as a second input.

### Revisit when

Phase 3 finds a case the notification misses.

---

## D-037: Status item order: NSStatusItem reachable without a fork; reorder persistence unverified

Status: Accepted. Date: 2026-10-05. Phase 0 spike.

### Context

v1.1 adds per-module tray items with ⌘-drag reordering, which needs a stable `autosaveName` per item.

### Decision

Reach the NSStatusItem through `TrayIcon::with_inner_tray_icon` and tray-icon's `ns_status_item()` and set `autosaveName` (for example `kelvo-cpu`) at creation. This works on Tauri 2.12.1 with no fork; Tauri asks for a pinned minor when using the inner handle, which D-030 already does. Whether a human ⌘-drag reorders and the order survives relaunch on macOS 27 could not be verified: synthetic ⌘-drags did not move the item, and writing the legacy `NSStatusItem Preferred Position` key had no effect for an unbundled binary.

### Consequences

Before v1.1 starts, a manual check on the bundled `Kelvo.app`: ⌘-drag the item, quit, relaunch, record the result here. Fallback if order does not persist: store a preferred order and recreate items in it.

### Revisit when

The manual check is done.

---

## D-038: `#[serde(other)]` does not skip unknown messages with content; decode peeks the tag

Status: Accepted. Date: 2026-10-05. Settles the open question in architecture.md infra 3 (phase 1, kelvo-proto).

### Context

`Message` is adjacently tagged (`{"t": ..., "c": ...}`) with a `#[serde(other)] Unknown` variant, so an older build can skip message types a newer peer sends. Whether that works when the unknown variant carries content was unverified.

### Decision

It does not. With serde 1.0.229 and ciborium 0.2.2, an unknown `t` decodes as `Unknown` only when `c` is absent or null. A map, array or scalar `c` fails with "invalid type: map, expected unit variant Message::Unknown", in either key order. Every real future message would carry content, so `serde(other)` alone is useless here.

`kelvo_proto::decode_message` keeps the attribute and adds the fallback the architecture named: decode normally, and only if that fails, decode just the `t` field. A tag not in `KNOWN_MESSAGE_TAGS` becomes `Message::Unknown`; a known tag with a broken body stays an error. The happy path decodes once. `tests/skew.rs` pins the raw serde behaviour (`raw_serde_other_rejects_unknown_variant_content`), checks the fallback for map, array, scalar, null and absent content, and keeps `KNOWN_MESSAGE_TAGS` equal to the enum's variants.

### Consequences

Adding a `Message` variant means adding its tag to `KNOWN_MESSAGE_TAGS`, a sample to `tests/skew.rs` and a fixture in `tests/fixtures/v1/`; the exhaustive match in the test fails to compile until the sample exists. Unknown fields inside known messages are already ignored by serde's default. Unknown values of closed enums inside known messages (a new `Module`, `GapReason` or `ModuleCap` from a newer peer) still fail the whole message; see the open item in D-039.

### Revisit when

serde changes adjacently tagged `other` handling (the pinned test goes red), or the decode cost of the fallback shows up in a profile.

---

## D-039: Serialized names: snake_case enum values, `JsSafeInt` for i64 over IPC

Status: Accepted. Date: 2026-10-05. Phase 1, kelvo-schema.

### Context

architecture.md sketches the types but not their serialized spelling, which is part of the wire format (one-way door) and of the generated TS bindings. The store also writes some enum values as text (`gaps.reason`, `gaps.module`). specta refuses to export `i64`/`u64` by default, and exports `f64` as `number | null` (NaN becomes null in JSON).

### Decision

- Every enum in `kelvo-schema` serializes its variants in snake_case (`"cpu"`, `"module_disabled"`, `"not_present"`, `"in_combined"`), the same text the store writes. Settings enums use descriptive names instead of the 6.4 abbreviations, which collide under snake_case (`MBps`/`Mbps` became `bytes_per_sec`/`bits_per_sec`, `GB`/`GiB` became `decimal`/`binary`, `C`/`F` became `celsius`/`fahrenheit`).
- `kelvo-proto`'s `Message` keeps the Rust variant names as `t` tags, as sketched.
- `Labels` serialize as an array of `[key, value]` pairs in key order and are re-sorted on decode; duplicate keys are a decode error. `SeriesKey` is `{ metric, labels }`.
- `i64`/`u64` fields that cross IPC (timestamps, `seq`, revisions, byte totals) carry `#[specta(type = kelvo_schema::JsSafeInt)]`, a marker that exports as plain `number`. The 2^53 bound is documented and tested in `kelvo-schema`.

### Consequences

The app shell's tauri-specta export needs no BigInt configuration. A new `i64` field without the marker, in any type that `crates/kelvo-schema/tests/specta_export.rs` registers or reaches, fails that test. New IPC-facing root types belong in its list. Open: closed enums give no forward compatibility on the wire. A v4 peer that adds a `Module` or `GapReason` breaks `Hello` or `SyncPage` decoding on older builds. An `Unknown` fallback variant would fix that, but it changes the TS types and the settings map, so it was not added without a decision. Settled by D-040: wire-facing and stored enums now have an `Unknown` fallback.

### Revisit when

Before v4 ships a second protocol version, decide on forward-compatible enum values (done in D-040).

---

## D-040: Wire-facing and stored enums get an `Unknown` fallback

Status: Accepted. Date: 2026-10-05. Settles the open item in D-039 (phase 1, before kelvo-store).

### Context

architecture.md infra 3 promises that mismatched versions interoperate. Unknown message types (D-038), unknown fields and unknown `metric_id`s were already skipped, but a closed enum value from a newer peer (a new `Module`, `GapReason`, capability state, OS) failed the whole `Hello`, `SyncPage` or `CapabilitiesChanged`. The store has the same problem in the other direction: `gaps.reason` and `gaps.module` are text, and a database written by a newer build can hold values an older build does not know.

### Decision

Every enum that crosses the wire or is stored as text has an `Unknown` variant that unseen values decode to:

- kelvo-schema: `Module`, `ModuleCap`, `UnsupportedReason`, `GapReason`, `Tier`, `OsKind`, `CoreKind`.
- kelvo-proto: `LiveTier`, `ErrorCode` (and `Message::Unknown`, D-038).

How: proto enums use `#[serde(other)]`. The schema enums cannot, because specta-serde refuses to export `#[serde(other)]` on an externally tagged enum, so they keep a derived `Serialize` and get `Deserialize` from a small helper (`compat::text_enum_deserialize!`) that maps any string not in the type's `ALL` list to `Unknown`; a test keeps `as_str` equal to the serialized text. `ModuleCap` carries content, where `serde(other)` would not help anyway (D-038), so it has a hand-written visitor: an unseen state decodes as `Unknown` whether it is a bare string or a map with any content; a known state with a broken body is still an error. `Unknown` is never in `ALL`, never produced locally, and serializes as `"unknown"`.

Consumers ignore it:

- `Capabilities.modules` and `Settings.modules` drop entries keyed by an unknown module on decode (`deserialize_with`), so they never show up as an `"unknown"` key in TS.
- `ModuleCap::Unknown` and `UnsupportedReason::Unknown` render as "not available".
- A gap with an unknown reason is still a gap: the store keeps it (`GapReason::from_stored` reads any unfamiliar text as `Unknown`) and charts draw a generic gap band. `Gap::validate` lets an unknown reason name a module, since a newer reason may be module-scoped. A gap whose module is unknown affects no known module (`Gap::affects`), and the store skips such gaps on sync ingest.
- `Tier::Unknown` has no bucket width (`bucket_ms`/`bucket_start` now return `Option`) and is never persisted; a sync request for it gets an error reply. `LiveTier::Unknown` is not served.

Not given a fallback, because they neither cross the wire nor are stored as text today: `Unit`, `MetricKind` (the catalog is compiled in and receivers look metrics up by ID), `Entitlement` (collector-side only), the alert enums `Condition`, `Cmp`, `ThermalState` (unused until v1.2; they get one when a message carries alert rules in v4), and the settings enums the user picks (`MenuBarMode`, `TemperatureUnit`, `NetworkUnit`, `MemoryUnit`, `Appearance`), which live only in the local settings file that Rust writes.

`crates/kelvo-schema/tests/forward_compat.rs` decodes an unseen value per enum from CBOR; `crates/kelvo-proto/tests/fixtures/skew/` holds hand-built v2-style `Hello`, `SyncPage`, `Subscribe`, `SyncRequest` and `Error` bodies that `tests/forward_compat.rs` decodes.

### Consequences

The TS bindings gain `"unknown"` in these unions once the types are exported; frontend code that switches on them needs a default arm. Adding a variant to one of these enums means adding it to the type's `ALL` and `as_str` (the round-trip test fails otherwise). Code that matches exhaustively on them has an `Unknown` arm to write, which is the point. A settings file from a newer build that uses a new settings-enum value still fails to decode; the app shell's settings loader has to fall back to defaults for it.

### Revisit when

A settings enum starts crossing the wire (v4 pushing settings to an agent), alert rules get a wire message, or `Unit`/`MetricKind` start travelling in a catalog exchange.

---

## D-041: kelvo-store details the architecture DDL left open: cursors, dedupe keys, process rows

Status: Accepted, amended by D-064 (each row kind travels only when negotiated; cursors store the kinds they cover). Date: 2026-10-05. Phase 1, kelvo-store.

### Context

architecture.md fixes the series model, the tier tables and the `(epoch, seq)` cursor, but leaves several store-level choices to the implementation: how gaps and events travel in a cursor read, how `Truncated` is detected after pruning, which keys make gap and event writes idempotent, and how process rows are packed and rolled down. These are part of what a sync peer observes, so they are written down here rather than left in code comments.

### Decision

Cursors and sync:

- A cursor read is per `(host, tier)`. Gaps and events have no tier; they ride the M1 cursor, so a page for M1 carries tier rows, gaps and events after the cursor's seq, and S10 pages carry tier rows only. `gaps` and `events` get `(host_id, seq)` indexes for this. Each kind is read only when the connection negotiated its `rows.*` feature, and the receiver's cursor records the kinds it covered (D-064).
- Every insert or change of a row takes a fresh `seq` from `meta.next_seq` in the same transaction, so a changed row (a gap closed, a bucket rewritten with a different blob) is re-sent to a peer behind it. An upsert that changes nothing (`WHERE blob IS NOT excluded.blob`) releases its seq, so replaying a page leaves identical rows and does not advance anything.
- A `pruned (host_id, tier, seq, ts)` table records the highest seq and timestamp deleted per tier. A cursor behind that seq gets `Truncated`; this includes a pruned gap or event with a higher seq than the cursor, since the reader cannot tell whether the peer saw it.
- On `Truncated` the receiver records a truncated gap over the lost span for M1 only (S10 data loss is covered by M1, and a gap row per tier would double-draw) and deletes its cursor, so the next read starts from zero. A changed `db_instance_uuid` (epoch) does the same: cursors are stored per `(peer host, tier)` with the peer's epoch in a controller-side `cursors` table, and `resume_cursor` ignores a row whose epoch differs from the peer's current one.
- Sync ingest skips gaps whose module is `Unknown` (D-040) and interns series and layouts on the receiving side; the sender's layout ids never cross.

Dedupe keys: tier rows upsert on `(host_id, bucket_ts, layout_id)` as architecture.md says. Gaps dedupe on `(host_id, start_ts, reason, module)` and events on `(host_id, ts, kind)`, done in the writer rather than with unique indexes, because `module` is nullable and SQLite treats NULLs as distinct. A gap that is closed is never reopened by a later write with `end_ts = NULL`; an older peer replaying an open gap cannot undo a close.

Writer:

- `upsert_host` commits immediately rather than waiting for the 30 s batch, because readers resolve `host_id` before anything else and a host that exists only in the writer's open transaction reads as `UnknownHost`.
- `begin_session` closes gaps left open by the previous run at the last persisted bucket end (or the gap's own start) and writes `app_not_running` from the later of the last bucket and the last gap end up to now.
- `clear_host` deletes the host's rows and its cursors and records the current seq in `pruned` for every tier, so peers behind it get `Truncated` instead of silently missing rows.

Process rows: a `proc_snap` blob holds the processes the engine passes for one instant (the top 30), 28 bytes per process (name id, pid, cpu, memory in KiB as u32, threads, idle wakeups, energy), so memory caps at 4 TiB per process. Snapshots older than 72 h roll down into `proc_top_1m`: per whole minute, the top 5 names by mean CPU across that minute's snapshots (a snapshot a process is absent from counts as 0), with mean wakeups and energy and peak memory and threads. Partial minutes at the cutoff wait for the next pass, so a minute is never split.

Queries: `history` with `TierChoice::Auto` reads S10 when the window is at most 24 h and S10 has not been pruned past its start, and M1 otherwise.

### Consequences

A peer that wants gaps and events must sync M1. Gap and event dedupe costs a lookup per write, which is negligible at their rate. Rows rewritten during a live session (the current bucket's blob changing) cost a fresh seq each time and are re-sent; that is correct, and buckets are written once when they close in practice. The 4 TiB memory cap is a format limit to revisit only if it is hit.

### Revisit when

A tier other than M1 needs its own gaps, a second writer process is considered (never, per architecture.md), or the process blob needs more fields.

---

## D-042: Vendor macmon's IOReport/SMC/HID code; SMC sensor keys from Stats; GPU split from IORegistry

Status: Accepted. Date: 2026-10-05. Phase 2, kelvo-collect (Apple Silicon collectors).

### Context

v1-local-monitor.md phase 2 asks to vendor macmon's private-API code instead of depending on the crate, and to confirm whether the GPU renderer/tiler split exists before `gpu.render`/`gpu.tiler` stay in the catalog. The SMC temperature keys differ per chip family, and macmon ships no per-family map.

### Decision

- macmon (https://github.com/vladkens/macmon, commit 98010ed, v0.8.2) is MIT licensed, Copyright (c) 2024 vladkens. That was checked against the LICENSE file in the clone, and it permits vendoring with the notice kept. The code lives in `crates/kelvo-collect/src/macos/vendor/`, with `LICENSE-macmon` beside it and an attribution header in `vendor/mod.rs` listing what changed: one file per source, per-group IOReport subscriptions, metadata read from samples, RAII release, no unwrap, `SAFETY:` comments. Copied rather than depended on because upstream leaks CF/IOKit objects, panics on a metadata mismatch, and has no per-group subscription builder. All `unsafe` stays in `vendor/`.
- The SMC temperature key lists for M1, M2, M3, M4 and M4 Pro/Max come from exelban/stats `Modules/Sensors/values.swift` (MIT, Serhiy Mytrovtsiy), credited in `macos/sensors.rs`. They are exact key lists, not prefixes: on an M3 Max, `Tp1g`..`Tp3j` sit pinned at 40.0 and `Tf06`/`Tf16` read 92 to 109 °C, so a prefix match would be wrong. Chips without a map (M5 and later, A-series) probe as `Unsupported(UnknownChip)`. `thermal.zone` from HID needs no map and keeps working there.
- The GPU renderer/tiler split is available. `IOAccelerator` (`AGXAcceleratorG15X` on M3 Max) exposes "Device Utilization %", "Renderer Utilization %" and "Tiler Utilization %" in `PerformanceStatistics`. Under a Metal compute load they read util 80–96, render 33–51, tiler 34–51, and all three read 0 at idle. This is a public IORegistry read (`Entitlement::None`). `gpu.render` and `gpu.tiler` stay in the catalog, so no catalog or Overview change is needed. The "(unverified)" note in the 7.1 catalog rows can go.

### Consequences

Upstream fixes to macmon have to be ported by hand. The header records the base commit so a diff is possible. The M1, M2 and M4 sensor maps are unverified on hardware until someone runs `live_smc_sensors` on those chips. M5 needs a map, and Stats has M5 keys to take when one is available to test.

### Revisit when

A non-M3 machine is available for the live tests, or Stats/macmon change their key lists.

---

## D-043: PMP energy counters refresh every ~5 minutes on macOS 27; PMP-derived power is a gap until they move

Status: Accepted; on the M3 Max `power.cpu` now comes from the SMC P-cluster keys calibrated to PMP (D-054), other chips keep this path. Date: 2026-10-05. Phase 2, kelvo-collect (IOReport collector).

### Context

`power.cpu`, `power.ane`, `power.dram`, `cpu.cluster.power` and `power.package` are planned as 1 Hz deltas of IOReport "Energy Model" counters. On the development M3 Max (macOS 27.0.1, Mac15,9), the PMP-published mJ channels (`EACC_CPU`, `PACC0_CPU`, `PACC1_CPU`, "CPU Energy", `ANE0`, `DRAM0`) changed only about every 5 minutes. A 15-minute watch saw updates after 255.7 s, 299.9 s and 302.3 s, worth 27.2, 18.3 and 16.8 W average CPU power. `PMP0` "Power state" reads INT64_MIN. "GPU Energy" (nJ, published by the GPU driver) updates every tick and tracks load (0.003 W idle, 22–25 W under a Metal compute load). A 1 Hz delta of a PMP counter is therefore 0 W almost always, then a multi-minute jump. Neither is the power over that tick.

### Decision

- `power.gpu` is emitted every tick from "GPU Energy".
- PMP-derived series (`power.cpu`, `power.ane`, `power.dram`, `cpu.cluster.power`, `power.package`) are emitted only when the CPU energy counter moved and the span since its previous move is at most 2.5 tick intervals (`MAX_PMP_SPAN_TICKS`). Otherwise the collector emits nothing for them and the engine records a gap. A frozen counter is not 0 W, and a 5-minute average is not a 1 Hz reading. On a machine whose counters update per tick (as macmon's users report on earlier macOS), behaviour is unchanged.
- `power.package` is the sum of CPU, GPU, ANE and DRAM, so it follows the PMP gating.
- Cluster definitions follow powermetrics, not macmon:
  - `cpu.cluster.active` is the HW active residency of the cluster complex channel.
  - `cpu.cluster.freq` is weighted over active states only, with the table minimum when idle.
  - macmon averages per-core residencies, which reads lower on a partly busy cluster. The future accuracy script has to compare like with like.

### Consequences

On macOS 27 the Power card, the component stack and the cluster-power rings have no CPU/ANE/DRAM/package values. Only `power.system` (SMC `PSTR`, 1 Hz, 6–120 W observed) and `power.gpu` are live. The UI has to render the gap state for those series. Unit tests cover both the frozen-counter and the multi-tick-jump case (mutation-checked).

Options not taken, open for a follow-up:

- Show the PMP average as a slow series (an EveryN(300)-like value with its real span). That needs a catalog entry, because it isn't the same metric.
- Derive CPU power as `power.system` minus the GPU and other parts. This is not measured and mixes in display/battery.
- Find a 1 Hz SMC CPU power key. Not explored: the scan was not run.

### Revisit when

A later macOS changes the refresh, the same behaviour is confirmed or ruled out on another chip, or the Power card design needs a value where this leaves a gap.

---

## D-044: Collector trait shape

Status: Accepted. Date: 2026-10-05. Phase 2.

### Context

Phase 2 needed the `Collector` trait before any collector could land, and the crate graph runs collect → engine, so shared tick types cannot live in the engine.

### Decision

`Collector: Send + 'static` with `id()`, `cadence()`, `modules()`, `required_entitlements()`, `probe()` and `sample(&Tick, &mut SampleBuf)`. `Tick { n, wall_ms, continuous_ns }` lives in kelvo-collect and the engine re-exports it. `CollectorId` is a `&'static str` newtype so each module owns its id. `Probe` has `Supported(Vec<SeriesKey>)`, `Unsupported { reason }` and `NotPresent`. `SampleBuf` is reused across ticks and drops non-finite values. `filter_by_entitlements` swaps a disallowed collector for a `Denied` stand-in that keeps its id and modules, probes `MissingEntitlement` and never samples, so the UI can still explain why a module is missing in an `appstore` build.

### Consequences

Every collector must override `modules()`; the default `&[]` exists only so partial work compiles. Collectors push only on their own sub-cadence ticks, which is why snapshots need a latest-value cache in the engine (phase 2 engine task).

### Revisit when

The engine needs per-collector state the trait cannot express.

---

## D-045: Public-API collector sources and process visibility without root

Status: Accepted. Date: 2026-10-05. Phase 2.

### Context

Several plan assumptions (sysinfo for CPU, `getifaddrs` for network, Q2's pressure definition) had to be checked against what macOS 27 actually exposes without privileges.

### Decision

CPU reads `host_processor_info` directly, because sysinfo has no user/system split; core P/E labels come from `hw.perflevel*`. Network reads `NET_RT_IFLIST2` for 64-bit counters and reports only Wi-Fi, Ethernet and cellular interfaces that are up and moving bytes, so VPN tunnels and AWDL do not double count. Memory uses Activity Monitor's definitions, and pressure is `100 − kern.memorystatus_level` (Q2 resolved; checked against `memory_pressure`, not Activity Monitor's graph); level 1/2/4 maps to 0/1/2. Battery fields come from `BatteryData`; macOS 27 has no battery `Temperature` key, so `battery.temp` is not probed (SMC `TB0T` is the candidate). Processes use libproc keyed by `(pid, start_time)`; without root only the user's own processes are readable (663 of 998 on the dev Mac), `compressed_bytes` is unavailable, energy comes from `ri_energy_nj` with a documented fallback, and wakeups use interrupt wakeups. `self.cpu` finds WebKit helpers by responsible PID, which needs no entitlement.

### Consequences

The Processes page states how many processes are hidden; showing them would need a privileged helper, which v1 does not build. Interface kind and the boot volume are exposed as collector methods until the schema has fields for them. CPU total was checked against `top` (11.3% vs 10.5% over different windows); disk rates were not load-tested.

### Revisit when

A privileged helper is considered (v3), or the schema gains interface and volume metadata.

---

## D-046: Engine input queue, Ticker and PowerSignals shape

Status: Accepted. Date: 2026-10-05. Phase 2, kelvo-engine.

### Context

architecture.md infra 6 sketches `Ticker::next()` as a blocking call and `PowerSignals::subscribe()` as a separate channel. The engine also has to react to pause, settings, device hints and shutdown while it waits for a tick, and on `WillSleep` it must flush before the machine sleeps. A blocking `next()` plus side channels leaves the engine deaf between ticks (up to 5 s) and makes the fake ticker's ordering against other inputs arbitrary.

### Decision

- Everything the engine reacts to arrives on one queue, the `Inbox`: ticks, sleep/wake, device hints and commands, handled one at a time in arrival order. The thread blocks on that queue, not on the timer. Tests run the same handler without a thread (`Engine::pump`).
- `Ticker` is `start(inbox, period, leeway)`, `stop()`, `now()`. A restart with a new period is how the interval changes. The macOS ticker is a GCD timer source on a serial utility-QoS queue with 10% leeway; its first tick lands on a wall-clock multiple of the period. Elsewhere a sleeping thread stands in until v4's `timerfd`.
- The inbox numbers ticks (`Tick::n`). A tick that arrives while the previous one is still unprocessed is skipped and does not take a number, so cadence multiples stay intact and a slow tick never builds a backlog.
- `PowerSignals` is `start(inbox)` plus `poll() -> PowerState`, called once per tick. Sleep and wake are events through the inbox; `WillSleep` carries a `SleepAck` that the engine answers after flushing buckets, opening the gap, flushing the writer and stopping the ticker. The macOS callback waits up to 10 s for it before `IOAllowPowerChange`. Battery, Low Power Mode, display sleep and screen lock are polled states (a notify(3) token for the power source, the rest at most every 2 s).
- macOS sleep/wake comes from `IORegisterForSystemPower` on a dispatch queue, not NSWorkspace, so it needs no AppKit run loop and works in the `dump` example.
- Events carry the clock reading taken when they happened. The sleep gap ends at `sleep wall time + continuous-clock span`, so an NTP step during sleep does not stretch or shrink it.
- More than 5 intervals of continuous time between ticks with no sleep event (a missed notification, a suspended process) becomes a `sleep` gap from the tick after the last one to the next tick. There is no better reason value for it, and the honest statement is that nothing was measured.
- Device hints (IOKit first-match and terminate notifications for `IOMedia` and `IONetworkInterface`) are coalesced and re-probe the affected modules' collectors on the next tick. Wake and resume re-probe everything, which also resets collectors' rate state. `EngineControl::reprobe` lets the shell ask for the same.
- One bus per host carries layouts, frames, capabilities, process batches and status. The sketch's separate `CapsPublisher` is a message type on it. Frames carry their layout, so a subscriber that lagged past a `Layout` message can still read later frames.
- Accumulators keep one partial row per layout seen in the bucket. Sleep, pause and shutdown flush the open bucket without resetting it; if ticks resume inside the same bucket, the next write is a superset that replaces the flushed row through the store upsert.
- Module switches act on series: a disabled module's series leave the layout, and a collector none of whose series remain is not sampled. `self.cpu` is not gated (catalog note). Turning off Power writes a `module_disabled` gap for Power and one for Sensors. Gaps the engine opened are closed on shutdown.
- Capabilities: `Available` beats `Unsupported`, which beats `NotPresent`. A collector that probes `Supported` with no series makes its modules `Available { series: 0 }`.

### Consequences

The trait shapes differ from the architecture sketch; the sketch is indicative, and this entry is the record. A remote source in v4 publishes into the same bus and store and does not use the ticker or power traits. When HID zones work on a chip without an SMC map, Sensors reads `Available` and the unknown-chip state does not show. `ModuleCap` cannot express both at once.

### Revisit when

The app shell needs to react to a power state faster than one tick, or the unknown-chip state needs to show next to live HID data.

---

## D-047: Frames carry raw and held values; the catalog carries a nominal cadence

Status: Accepted. Date: 2026-10-05. Phase 2, kelvo-engine.

### Context

Collectors push values only on their own cadence. Disk capacity arrives every 60 ticks, load average every 5, and PMP power only when its counters move (D-043). A Snapshot built from one frame shows those series as `None` 59 seconds out of 60. Holding the last value inside the frame would fix the display but put repeated readings into the ring buffer and the rollups, which must only ever see measurements. Some series are also sampled less often than their collector's cadence: `fan.max` sits inside the every-2-ticks fan collector but is read every 60.

### Decision

- `LiveFrame` has two arrays. `values` holds this tick's measurements, `NaN` elsewhere; the ring buffer, backfill and the accumulators read it. `held` is a latest-value cache: a value stays current for 2.5 sampling intervals and is `NaN` after that. `LiveFrame::snapshot` builds the typed Snapshot from `held`.
- The sampling interval of a series is the larger of its catalog cadence and its collector's current cadence (Adaptive counts its current mode), times the base tick in effect when it was sampled. `MetricDef` gains `cadence: u16`, the 6.1 Cadence column, set with `.every(n)` in the catalog.
- A series that stops arriving is `NaN` in `held` after two missed samples, so the never-interpolate rule still reaches the UI. A PMP power value (catalog cadence 1) shows for 2.5 s after a counter move and is a gap otherwise, as D-043 intends.

### Consequences

Frames are about twice as large in memory (an extra `Arc<[f32]>` per tick, about 700 bytes at 170 series). Consumers have to pick the right array: draw lines from `values` or backfill, read current numbers from `held`. The frontend live store gets frames with nulls and needs its own rule for current-value readouts; phase 3 decides whether `LiveMsg::Frame` also carries the held values.

### Revisit when

A collector gets a cadence that varies at runtime beyond Adaptive, or the frontend wants the held array over IPC.

## D-048: Bindings export in unified mode, not serde phases

Status: Accepted. Date: 2026-10-05. Phase 3, app shell.

### Context

tauri-specta rc.25 exports with serde phases by default. Any type that reaches a `deserialize_with` field (`Capabilities`, `Settings`) is split into `_Serialize` and `_Deserialize` aliases, and in that mode the internally tagged `LiveMsg` came out externally tagged (`{ Frame: {...} }`), which is not what serde sends.

### Decision

`specta_builder()` calls `.disable_serde_phases()`. Every Rust type gets one TS type. No Kelvo type has a different shape per direction, so nothing is lost. A test in `src-tauri/src/ipc.rs` reads the generated file and checks that `LiveMsg` is flat (`{ kind: "frame"; ts_ms: number; ... }`) and that no `_Serialize` alias exists. Command arguments and results that are `i64`/`u64` use the `Millis` and `ByteCount` newtypes so they export as `number` under `JsSafeInt`.

### Consequences

Optional `f32` values export as `number | null`. A type that one day needs a different shape per direction has to be two Rust types.

### Revisit when

A tauri-specta release exports tagged enums correctly in phased mode and a type needs the split.

## D-049: Live channel protocol: frames carry raw and held values, resume backfills only the missed span

Status: Accepted. Date: 2026-10-05. Phase 3, app shell. Settles the open item in D-047.

### Context

D-047 left open whether `LiveMsg::Frame` carries the held values. The plan also asks the channel to stop while a window is hidden and resume with a fresh backfill, and `set_process_interest` to be reference-counted per window.

### Decision

- `LiveMsg::Frame` carries both `values` (this tick's measurements, for chart lines) and `held` (D-047's latest-value cache, for current-number readouts). `NaN` crosses as `null` in both. Backfill rows stay raw. The cost is about 1 KB more per frame per visible window, and only visible windows get frames.
- `subscribe_live(host, channel, backfill_ms)` sends `Caps`, `Status`, then per ring segment a `Layout` (when the layout changed) followed by its `Backfill`, then live frames. Frames already covered by the backfill are dropped, so nothing arrives twice. A `Layout` precedes the first frame of any new layout. A resubscribe from the same window replaces its stream.
- The registry is keyed by window label and host. `window_visible(label, false)` drops the stream's bus subscriber; `true` resubscribes and backfills only from the last row the window received (capped at the ring span). The engine's `display_idle` status pauses frames the same way, and display wake catches up. An unknown label counts as visible.
- Process interest is a per-window wish. The engine count includes a window only while it is visible, so hiding a window releases its interest and showing it restores it without the frontend calling again. Closing the window clears it.

### Consequences

The frontend never has to ask for a backfill after a hide; it receives one. The window and tray code must report visibility (hide, minimize, occlusion) through `window_visible`; the shell does not observe those itself.

### Revisit when

Frame size matters (remote hosts over a network in v4), or a consumer wants held values in backfill.

## D-050: Settings owner shape

Status: Accepted. Date: 2026-10-05. Phase 3, app shell.

### Decision

- `get_settings` and `update_settings` return `SettingsSnapshot { revision, settings }`. The revision is in memory, starts at 1 each run, and goes up by one per applied change; `settings-changed` carries the same pair. A no-op patch does not bump it.
- `update_settings` takes a typed `SettingsPatch` with optional sections. It is applied to a copy and validated with `Settings::validate`; a module that has no entry is rejected.
- Order under one lock: apply and validate, side effects that can fail (the login item), write `settings.json` through tauri-plugin-store, commit the revision, then apply to the engine and emit. A failed save leaves the in-memory settings unchanged. Engine-affecting changes are queued on the engine's inbox before the event fires.
- A missing settings file is a first run with defaults. An unreadable or invalid one falls back to defaults with a warning and is overwritten on the next change.

### Revisit when

A second writer (a CLI, a v4 controller) needs revisions that survive restarts.

## D-051: Shell housekeeping: host id, logs, login item, pruning

Status: Accepted. Date: 2026-10-05. Phase 3, app shell.

### Decision

- **Host id.** `host-id` in the app data dir wins. If it is missing or corrupt, the shell reuses the store's `is_local` host, and only then creates a v4 UUID. Writes are atomic (temp file then rename). `chip_known` starts as "brand starts with Apple M" and is set false (and upserted) when the engine reports Sensors `Unsupported(UnknownChip)`.
- **Host info in the shell.** The sysctl and computer-name reads are in `src-tauri/src/platform/`, not taken from kelvo-collect, because the shell may depend only on engine, store and schema.
- **Logs.** `tracing-appender` rotates by time only: daily files `kelvo.YYYY-MM-DD.log`, five kept. Debug builds also log to stderr. `RUST_LOG` overrides the `info` default.
- **Launch at login.** `SMAppService.mainApp`. Outside a `.app` bundle (`cargo run`, `tauri dev`) it is a logged no-op. At startup the shell reconciles the registration with the setting only after onboarding is complete.
- **Pruning.** The shell runs `Writer::prune` 2 minutes after launch, then hourly, and right after a retention change.
- **Not yet real.** `check_for_updates` returns `NotConfigured` until the updater lands (phase 6). `sensor_dump` returns host info plus the current Power and Sensors series; enumerating SMC, HID and IOReport keys needs a kelvo-collect API (phase 5).

## D-052: Light muted-foreground is #63636b, not zinc-500

Status: Accepted. Date: 2026-10-05. Phase 4, frontend foundation.

### Decision

Light `--color-muted-foreground` changes from `#71717a` to `#63636b`. The phase 4 axe check found `#71717a` passes 4.5:1 only on white (4.83) and `#fafafa` (4.63). It fails on the surfaces muted text actually sits on: `--color-deep`/sidebar `#f4f4f5` (4.40), the selected sidebar row `#e8e8e9` (3.95), the primary-tinted tray option `#ebf6f8` (4.39) and `--color-raised` `#e4e4e7` (3.81). `#63636b` is 5.95 on white, 5.42 on `#f4f4f5`, 4.86 on `#e8e8e9` and 4.69 on `#e4e4e7`, and stays well apart from `--color-fg-subtle` `#3f3f46`. Dark is unchanged (`#8a8f98` passes everywhere it is used).

The original mock token sheet listed `#71717a` and a 4.8 ratio measured against white.

### Revisit when

A new light surface darker than `#e4e4e7` carries muted text; the Playwright check in `tests/e2e/gallery.spec.ts` will fail first.

## D-053: Frontend live state and dev entry points

Status: Accepted. Date: 2026-10-05. Phase 4, frontend foundation.

### Decision

- **Readouts before the first frame.** A `Backfill` seeds `held` from its last row for series not already held, so the popover shows numbers on open instead of blanks for one tick. The next `Frame` replaces `held` whole (D-047).
- **Row ring mutated in place.** `reduceLive` appends rows to a per-host `RowRing` (one hour) without copying, and bumps `rowsVersion`. Charts subscribe to `rowsVersion`; readouts subscribe to `held` through per-module `useShallow` selectors. A row not newer than the ring's last is dropped, which covers a resumed channel's overlap.
- **Stale.** The host store marks a host stale after three intervals with no frame while not paused, and clears it on the next frame or a paused `Status`.
- **Mock transport from the URL.** In a browser the app builds the mock transport, configured by `?window=` (label), `?scenario=a,b` and `?ticks=0`. Inside Tauri it uses the Channel transport unless `VITE_TRANSPORT=mock`.
- **Dev route override.** In dev builds only, `?route=/...` overrides the label's entry route and `?theme=light|dark` pins the theme. `/dev/gallery` is registered only in dev builds.

### Revisit when

Remote hosts (v4) add a second store, or a window needs more than one hour of live rows.

## D-054: On the M3 Max, CPU power comes from the SMC P-cluster keys, calibrated live to PMP

Status: Accepted, option (b), chosen by the user. Date: 2026-10-05. Phase 2, kelvo-collect (SMC collector). Follows D-043. Amended by D-065: the scale persists per chip across restarts and new sessions start seeded.

### Context

D-043 leaves `power.cpu`, `cpu.cluster.power` and `power.package` as gaps on macOS 27 because the PMP counters refresh every ~5 minutes or slower (refresh windows of 352 s, 918 s and over 1,400 s were seen during this work, once over 13 minutes frozen). The user approved a read-only SMC scan (key info and read commands only, never a write). On the M3 Max (Mac15,9, macOS 27.0.1) `#KEY` lists 2,877 keys, 88 starting with `P`. The keys other tools use for CPU power (`PCPC`, `PCPT`, `PC0C`) do not exist on this machine.

Method: every `P` key sampled at 1 Hz under alternating loads (idle, 1 to 12 CPU threads pinned by QoS, a GPU-only Metal load), then the candidates compared with the PMP cluster counters (`EACC_CPU`, `PACC0_CPU`, `PACC1_CPU`) over whole PMP refresh windows, which is the only span the PMP average is valid for.

### Findings

- **P clusters.** `PC02` + `PC03` follow P cluster 0 and `PC42` + `PC43` follow P cluster 1. A single thread migrating between clusters moves its watts from `PC02` to `PC42` in the same second. All four read 0 under the GPU-only load.
- **Not CPU.** `PC10`, `PC12`, `PC20` and `PC22` (about 10 W each under load) follow the GPU. `PSVR`, `PHPC`, `PZC0`, `PZC1`, `PHPS`, `PE0b`, `PDTR` and `PSTR` follow both CPU and GPU (SoC or system rails; `PSTR` lags about 1 s).
- **Accuracy.** SMC means against PMP over the same refresh window:

  | Window | P0 SMC / PMP | P1 SMC / PMP |
  |---|---|---|
  | light load, 918 s | 1.855 / 2.459 W = 0.75 | 1.102 / 1.387 W = 0.79 |
  | 2 steady threads, 352 s | 4.969 / 6.622 W = 0.75 | 5.016 / 6.731 W = 0.75 |

  The SMC keys read about 25% low. PMP is what powermetrics and macmon report, and verification.md asks for ±5%.
- **E cluster not found.** `PPMC` reads 1.249 W against PMP 0.304 W and 0.795 W against 0.130 W, so it is not the E cluster. `PP5b` reads 0.181 against 0.304 and 0.088 against 0.130 (0.6 to 0.68), and about 0 W with the E cluster fully busy at 1 GHz. No key tracks the E cluster.
- **Cost.** Reading the six keys every tick added about 0.02% CPU (parallel A/B run, D-055).

### Options considered

- **(a) Fixed per-chip scale.** Multiply the P-cluster keys by about 1.33 (four window ratios, 0.75 to 0.79). Cheap, but the scale comes from one machine and two loads.
- **(b) Live calibration.** Use the SMC keys for 1 Hz shape and rescale them so their energy over PMP refresh windows matches PMP.
- **(c) 5-minute PMP average shown as such**, as a separate `power.cpu_avg` series, with `power.cpu` a gap.

### Decision

Option (b), on verified chips only (the M3 Max today, `cpu_power_map`); every other chip keeps D-043's PMP path.

- **Source.** The SMC `Power` collector (probed before IOReport) reads `PC02`+`PC03` (P0) and `PC42`+`PC43` (P1) every tick and emits `cpu.cluster.power{cluster=P0,P1}` and `power.cpu` as their sum. IOReport then leaves those series to it but keeps the PMP channels subscribed for ANE, DRAM and package power and for the calibration.
- **E cluster left out.** No key tracks it (`PPMC` reads 4 to 6 times the PMP E-cluster energy), so `power.cpu` on these chips is the P clusters only and there is no `cpu.cluster.power{cluster=E0}`. The E cluster is 0.1 to 0.3 W at light load and up to about 1 W fully busy.
- **Calibration** (`kelvo_collect::calib::Calibrator`, shared through `CpuPowerSource`). The SMC collector integrates P-cluster watts times the time since its previous sample; IOReport reports the P clusters' PMP energy whenever the counters move. A window runs from one observed move to a later one, at least 60 s long, and closes only when the slow sample period is at most 5% of it (10 s tray-only against a 300 s refresh is 3%). The window's ratio, PMP energy over SMC energy, is clamped to 0.5 to 2.0 and folded into the scale with weight 0.5. A fast step over 2.5 s (sleep, or an interval of 5 s or more, where one reading no longer stands for its step) or a missing reading drops the open window; the next move starts a new one. The scale survives re-probes and, since D-065, restarts: each closed window saves it per chip through a `ScaleStore` the shell owns (`power-calibration.json`), and the next session starts from it, or from the chip's measured default (1.33 on the M3 Max) before any calibration.
- **Uncalibrated is visible.** Until the first window closes (two PMP refreshes on macOS 27: 10 to 30 minutes when they refresh every 5 to 15 minutes, but in the first hardware run the counters moved once in 45 minutes, so no window closed) values were unscaled, about 25% low; since D-065 they are scaled by the seed instead. `power.cpu_source` (enum, ring only) is 1 with no scale, 3 while scaled by a seed (D-065) and 2 once a window closed this session; it is absent where `power.cpu` comes from PMP. The UI should label the value with it ("P cores", "calibrating").
- **Tests.** Calibrator tests with synthetic windows: no scale before the second move, varying load, 10 s tray-only slow sampling, coarse resolution waiting for a longer window, counters that move every tick, clamping and smoothing, idle windows, sleep and missing readings dropping the window. A `#[ignore]` hardware test (`live_cpu_power_calibration_matches_pmp`) runs both collectors at 1 s and checks calibrated SMC energy against PMP over whole windows that started after calibration, within ±5%. Its first run (2026-10-05, idle-to-light load) was stopped at 45 minutes: one PMP move at 1,148 s and none after, so neither a calibration nor the ±5% check happened. Calibrated accuracy on hardware is unverified.

### Consequences

`power.cpu` is 1 Hz on the M3 Max from launch, accurate to PMP after the first two refreshes, which can take well over half an hour when the counters stall, and never includes the E cluster there. `power.package` stays PMP-based (a gap between moves) because it includes the E cluster. A user who sets an interval of 5 s or more never calibrates and keeps the last scale only if one was learned earlier in the run. The cost is about 0.02% CPU for the reads (parallel A/B run) and one mutex per tick, uncontended.

### Revisit when

A second chip is available to check whether the 0.75 ratio is per-chip, or a later macOS restores 1 Hz PMP.

## D-055: Temperatures are sampled every 5 ticks

Status: Accepted. Date: 2026-10-05. Phase 2, kelvo-collect and kelvo-schema.

### Context

The release engine measured 1.39% CPU at idle in the phase-2 log, against the app-wide 0.5% budget. About 90% of it is system time in IOKit calls. Per-call cost was measured at the 1 Hz call rate the engine uses (a tight loop under-reports it 3 to 10 times because the kernel paths are cold each second):

| Call | Cost | CPU at its cadence |
|---|---|---|
| SMC, 29 mapped sensor keys (cached key info) | 3.0 ms | 0.15% every 2 ticks |
| HID, all 46 temperature services | 3.0 ms | 0.15% every 2 ticks |
| IOReport, CPU cluster channels | 3.9 ms | 0.39% every tick |
| IOReport, all subscribed groups as shipped | 5.1 ms | 0.51% every tick |
| IOReport, without the PMP energy channels | 4.7 ms | 0.47% every tick |

Already in place before this change: SMC key info cached per key, only mapped SMC keys read, the HID client and service list opened once and refreshed on probe/wake, the IOReport subscription filtered to the channels in use with no per-sample CF re-creation.

### Decision

- `thermal.zone`, `thermal.cpu`, `thermal.gpu`, `thermal.hottest` and `thermal.sensor` are sampled every 5 ticks (`Cadence::EveryN(5)` in `HidThermal` and `SmcSensors`; `.every(5)` in the catalog, so a held value stays valid for 12.5 ticks). Die temperatures move over seconds; the tray's `thermal.hottest` is at most 5 s old (10 s on battery at the 2 s base tick).
- `fan.rpm` and `thermal.state` stay every 2 ticks.
- CPU/GPU utilisation, frequency and residency stay every tick.
- The PMP energy channels stay subscribed: dropping them saves about 0.04% (inside run-to-run noise) and loses ANE, DRAM and package power whenever PMP does refresh.
- HID services with duplicate names are distinct sensors (the two PMUs read different values) and are all read.

### Consequences

Measured in parallel A/B runs (300 s, both variants at once, so background load hits both): 0.957% → 0.803%, 0.757% → 0.653%, and on the committed tree 0.990% → 0.830%, about −0.15 percentage points (−16%). The engine is still above the 0.2% soft target: IOReport CPU cluster channels alone cost about 0.4% at 1 Hz, so 0.2% is not reachable while cluster frequency and residency are sampled every tick.

### Revisit when

A design needs sub-5 s temperatures (a thermal throttling view), or the IOReport cluster read gets an adaptive cadence (for example every tick only while a window is open), which is the remaining large lever.

## D-056: Tray, popover panel and window lifecycle details

Status: Accepted. Date: 2026-10-05. Phase 3, tray, popover and windows.

### Decision

- **Tray image size.** tray-icon always shows the image 18 pt tall and scales the width to match. The renderer draws at 18 pt × scale. Scale is 2 if any display is Retina, because AppKit downsamples a 2x image cleanly on a 1x display and blurs a 1x image on Retina.
- **Frame skip by equality, not a hash.** The quantized `TrayFrame` is small (bar heights in device pixels plus a few short strings), so comparing it to the last frame drawn is exact and costs less than hashing it. Quantization: device-pixel bar steps, 1% for percentages, 1° for temperatures, 0.1 W, compact rates such as "38.4MB". Debug counters log drawn, skipped and display-idle frames every 60 s.
- **Fixed width.** Each text item reserves a minimum number of characters and is right-aligned: 3 for percentages and temperatures, 5 for PWR, 6 for NET and DSK. Without that, the status item changes width with the values (a paused "–" against "52°") and shifts every item to its left.
- **Layout.** Elements within the combined glyph are 4 pt apart and groups are 10 pt apart. The temperature sits after the bars in the combined glyph, or appears as a labeled SOC value when no bars show. Watts show as PWR. Paused or gap readings draw the bar track only and a dash for text. If every element is hidden, three empty tracks keep the item clickable.
- **Clicks.** Control-click arrives from tray-icon as a left click. The shell checks `NSEvent.modifierFlags` on Left Down and shows the menu through the status item. The tray thread runs on the bus `Subscriber` with no timer of its own, so it follows the battery back-off.
- **Popover hides on `WindowEvent::Focused(false)`.** The nspanel event handler is not used for this. A second tray click first takes focus and hides the panel, so a click within 350 ms of a hide does not reopen it. Esc is a local key-down monitor on the panel.
- **Placement.** Centered under the status item, 4 pt below it, clamped 8 pt inside the screen's visible frame (which excludes the menu bar and notch area), and shortened if the screen is too short.
- **Open-latency probe needs no page code.** Rust records the click time and evals a double `requestAnimationFrame` that invokes `report_popover_paint` with a token. The debug log reports each open and the p95 over the last 50.
- **WebContent termination.** A showing popover is flagged and reloads on its next hide. Any other webview reloads at once.
- **Dashboard lifecycle.** Close is prevented. The window's state is saved, it hides, and a generation counter starts a 5-minute timer; the window is destroyed only if no show happened in between. tauri-plugin-window-state remembers size and position, with the popover and onboarding on its denylist. A new dashboard loads its route as the URL path. An existing one gets a `navigate-requested` event sent only to it. `open_dashboard` accepts only `/dashboard` routes.
- **Onboarding.** 820 × 566, not resizable, overlay title bar, centered. The tray is created at launch even before onboarding completes.

### Not verified

- The D-033 fullscreen first-show anomaly was not reproduced (it would take over the user's display with a fullscreen Space). `full_screen_auxiliary` and `can_join_all_spaces` are set as the spike found.
- The live Reduce Transparency switch is not exercised.
- WindowServer cost is estimated from a separate status item at 60 and 120 Hz, not measured from Kelvo itself.

### Revisit when

v1.1 adds own-item modes (one status item per module, each with its own frame skip) or a display change needs the tray scale updated without a restart.

## D-057: History stays under a byte cap and stops growing on a nearly full disk

Status: Accepted. Date: 2026-10-05. Phase 6 prep, kelvo-store and the shell's pruner.

### Context

Retention was time-only. The fill test showed what that means: 30 days at 150 series is 142.6 MB, but 250 series is 249 MB, and the WAL reached 31.8 MB (150 series) and 55.0 MB (250 series) because pruning never truncated it, so the real on-disk peak at 150 series was about 174 MB. A history file should never be the thing that fills a Mac's disk, whatever the series count or retention setting.

### Decision

- **Byte cap.** `Retention::max_bytes`, default `Retention::DEFAULT_MAX_BYTES` = 150 MB (10^6 bytes), the architecture budget. It is a constant, not a setting: 6.4 has no natural place for it, and the Settings page already shows size and retention. After time-based pruning the writer runs `incremental_vacuum`, checkpoints with `TRUNCATE`, and measures the database plus `-wal` and `-shm`. If that is over the cap, it deletes the oldest history on every host (tier_1m, proc_top_1m, gaps that ended, events, and any tier_10s / proc_snap rows) up to a minute-aligned cutoff, in slices sized from the excess, until the live pages (`page_count - freelist_count`) are under a low-water mark of 90% of the cap. Live pages, not file sizes, decide when to stop, so a reader blocking the checkpoint cannot cause over-trimming.
- **Never interpolate.** The trim always cuts from the start of history, so the removed span reads as "no data before `earliest_ts_ms`", like a fresh install, and no gap row is written. Gaps straddling the cutoff stay. The `pruned` marks for M1 and S10 move to the cutoff, so a sync cursor behind it gets `Truncated` (D-041).
- **Floor.** The trim never cuts into the last 24 hours. If that is not enough, `CapTrim::cap_met` is false and the shell logs an error.
- **Report.** `PruneReport` gains `cap_trim: Option<CapTrim>` (new earliest timestamp, size before, rows per table, `cap_met`) and `size_bytes` after the checkpoint. The shell logs "history trimmed to stay under 150 MB" and keeps `HistoryHealth { low_disk_paused, trimmed_before_ms, cap_met }` on `History::health()`. It is not in the IPC bindings yet; adding it needs a command or a field in `commands.rs`/`ipc.rs`.
- **Page size 16 KiB** for new databases (pre-v1, no migration). Measured with the fill test (31 days, pruned daily, size after close):

  | page_size | 150 series | 250 series | WAL bytes per 30 s commit (150 / 250 series, steady state) |
  |---|---|---|---|
  | 4096 | 142.6 MB | 249.2 MB | 59.7 KB / 64.1 KB |
  | 8192 | 139.5 MB | 245.9 MB | 93.7 KB / 103.1 KB |
  | 16384 | 139.7 MB | 203.4 MB | 154.2 KB / 167.4 KB |

  A 250-series M1 blob is 3,000 B, which fits once per 4 KiB page and five times per 16 KiB page. That is an 18% cut at 250 series and 2% at 150. The cost is about 2.6 times the write volume: roughly 0.44 GB a day of WAL at 150 series instead of 0.17 GB, which is far below SSD endurance concerns. `wal_autocheckpoint` is set to 256 pages (4 MiB) so the WAL does not grow four times bigger with the larger pages.
- **Low-disk guard.** `kelvo_store::LowDiskGuard` reads free space through a `FreeSpace` trait (the real one is `statvfs` via `rustix`, which leaves out APFS purgeable space and so errs towards less free). When available space drops under the smaller of 2 GB and 5% of the volume, `Writer::set_s10_paused(true)` makes the writer drop S10 buckets before queueing them; M1, gaps, events, process snapshots and the in-memory ring continue. It resumes above 1.5 times the threshold, so a volume sitting at the line does not flap. The shell checks every 5 minutes and after each prune (one `statvfs`, never per tick). The pause span is recorded as `meta.s10_hole_until` (`i64::MAX` while paused, the resume time after), and `TierChoice::Auto` reads M1 for any range starting before it, so a chart never draws a 10 s tier with silent holes. A run that ends while paused gets the hole closed at the next open.

### Consequences

At 250 series the cap keeps about 17 days (fill test: 24,999 M1 rows, 139.9 MB after close, at most 150.0 MB right after any prune). At 150 series nothing is trimmed (139.7 MB). The retention setting is now an upper bound, so the Settings copy for 90 days should say the cap may shorten it. The S10 hole marker is a single span: after a pause, ranges from before the resume read M1 until the 24 h S10 window has moved past it, which is conservative and correct. Free space checks cost one syscall every 5 minutes.

### Revisit when

Users want a configurable cap, v4 per-host mirror budgets arrive (v4-remote-hosts.md), or a sync peer needs the S10 hole span (today it is local metadata only).

## D-058: Kelvo has zero runtime dependencies to install

Status: Accepted. Date: 2026-10-05. Phase 6 prep, packaging.

### Decision

Kelvo needs nothing installed besides itself. Everything comes from macOS APIs or is compiled or bundled in: SQLite is the `rusqlite` bundled build, fonts ship in the app, the IOReport/SMC/HID code is vendored from macmon (D-042), and the binary links only system frameworks and `/usr/lib` libraries (`libIOReport`, `libSystem`, `libobjc`, `libiconv`). Dev-only tools, such as macmon for `scripts/accuracy-vs-macmon.sh`, are optional and never needed at runtime. Any future dependency must be bundled inside the .app or installed by the app itself on first run with the user's consent; asking users to run `brew install` is not an option.

`scripts/check-deps.sh` (`make check-deps`, and a CI step after the macOS debug build) runs `otool -L` on the built binary and fails if any linked library is outside `/System/Library/` and `/usr/lib/`, including `@rpath` entries. Bundling a library on purpose means extending that allowlist in the same change.

### Consequences

New crates with C dependencies must use a vendored or bundled build feature. The check covers dynamic linking only; a library opened with `dlopen` at runtime would not show up, so that needs the same review by hand.

### Revisit when

A feature truly needs a component macOS does not ship and that cannot be statically linked; then it gets bundled in `Contents/Frameworks` and the allowlist names it.

## D-059: History size limit is a setting; the store's health reaches the UI

Status: Accepted. Date: 2026-10-05. Phase 6.4 (Settings), supersedes the "constant, not a setting" part of D-057.

### Context

D-057 fixed the byte cap at 150 MB and kept `HistoryHealth` inside the shell. Users with many series or 90-day retention hit the cap without being told, and someone with disk to spare could not trade it for more history. The UI also had no way to show the low-disk pause or a trim.

### Decision

- **Setting.** `HistorySettings::size_limit_mb`, one of `SIZE_LIMITS_MB` = 150 (default), 300, 500 and 1000 MB (10^6 bytes). It is validated like the other enums (`SettingsError::SizeLimit`, D-050), and `retention_for` maps it to `Retention::max_bytes`. Any change to `history` (retention or limit) wakes the pruner immediately (`prune_affecting`), so lowering the limit trims right away instead of at the next hourly prune.
- **Projection.** Settings shows a projected size next to each retention option and, when the limit cuts retention short, "Limited to about N days by the X MB limit". Onboarding step 2 quotes the same model. The model is `src/core/history-projection.ts`: a fixed part that scales with series count plus a per-day part that is linear in series count, fitted to the three D-057 fill-test points (150 series/30 days 139.7 MB, 250/30 203.4 MB, 250 under 150 MB = 17.4 days). Days under a limit are quoted at 95% of it, halfway between the 90% low-water mark and the limit. The series count is the live layout's, or 150 before the first frame. Other series counts are extrapolated, which is why the copy says "about".
- **Health over IPC.** `HistoryHealth { low_disk_paused, trimmed_before_ms, trimmed_limit_bytes, cap_met }` moves to `ipc.rs`, with a `history_health(host)` command (`HistoryUnavailable` when there is no store) and a `history-health-changed` event that the pruner emits only when the value changes. A prune without a trim sets `cap_met` back to true. The trim note is cleared by Clear history, or once the trim point falls out of the minute retention, because then retention, not the limit, decides where history starts. Health is in memory: a restart forgets a trim until the next one.
- **Notices, without crying wolf.** `historyHealthNotices` in `src/core/history-state.ts` decides what to show. The low-disk pause and an unmet cap are warnings (bordered tile, icon, `role="alert"`). A trim is informational (one muted line, `role="status"`) and on a history view is shown only when the view's range reaches before the trim point. Settings shows all of them under the history card. Timeline and the battery card render `HistoryUnavailableBanner` when the store is unavailable.
- **Sampling interval UI** (with D-061). Settings offers all seven intervals with the overhead sentence, and the battery row names the backed-off interval. Live chart windows in the popover and the Overview GPU card scale with the interval (`scaledWindowMs`: base window times interval/1 s, capped at the 1 h ring), so a 30 s interval shows 30 minutes instead of two points. The popover's first backfill is sized the same way. Module window controls disable windows that would hold fewer than 10 samples and move the selection to the shortest one left. The tray already redraws on each bus frame, so it follows the interval without changes.

### Consequences

The retention setting is an upper bound whose real reach depends on the limit and the series count, and Settings now says so. 10-minute windows (power stack, core heatmap, swap, GPU power, battery power) and the 60 s GPU frequency and CPU residency cards keep fixed windows and their "Last 60 s" labels; at 30 s and 60 s they hold few points.

### Revisit when

A user needs a limit outside the list, health has to survive a restart (store it in `meta`), or v4 per-host mirror budgets arrive.

## D-060: Frontend performance gate in Playwright, thresholds in `perf-budget.json`

Status: Accepted. Date: 2026-10-05. Phase 7 (performance).

### Decision

`tests/e2e/perf-gate.spec.ts` opens the popover and the Overview on the dev server with the mock transport at 1 Hz, waits 5 s, then samples Chromium's CDP `Performance.getMetrics` for 20 s. It asserts that `ScriptDuration + LayoutDuration + RecalcStyleDuration` per second of wall time stays under the screen's budget, and that no long task over 50 ms starts after warm-up. Chromium only, since WebKit has no CDP. A Vite dependency reload resets the counters, so a sample that spans one (a changed `performance.timeOrigin`) is taken again, up to three times.

Thresholds live in `perf-budget.json` at the repo root, one file for every automated perf gate (the engine gates add their own section). Frontend: popover 40 ms/s and Overview 50 ms/s. Measured on an M3 Max dev machine: popover 15.4 to 20.0 ms/s, Overview 16.2 to 25.5 ms/s, no long tasks, across runs alone and in the full suite. The budget is about twice the worst measurement.

**Raising a threshold needs a decision entry** that names the change that made the screen more expensive and why it is worth it. Lowering one does not.

### Consequences

The gate catches gross regressions, not small ones: a deliberate mutation that re-renders the whole popover every tick measured 21 to 23 ms/s, inside the budget. Narrow-selector discipline stays the job of the render-count tests (`rules/frontend/testing.md`). The numbers are dev-mode React in Chromium, not WKWebView in a release build, so they are a relative signal, not the idle CPU budget in `architecture.md`, which stays measured with `scripts/bench-vs-stats.sh`.

### Revisit when

CI runners give numbers far from the dev machine's (move to a ratio against a baseline page), or a production-build Playwright target exists (measure that instead).

## D-061: Sampling intervals from 0.5 s to 60 s; collector cadences are wall-clock periods; tray-only IOReport at 10 s

Status: Accepted. Date: 2026-10-05. Phase 6.4 and 7 (performance).

### Decision

- **Intervals.** `SamplingSettings::INTERVALS_MS` is 500, 1000, 2000, 5000, 10000, 30000 and 60000 ms; the default stays 1 s. Battery back-off doubles the chosen interval, capped at 60 s (`backoff_interval_ms`), instead of a fixed 2 s.
- **Cadences are wall-clock minimum periods, not tick multiples.** `Cadence::Every(ms)` and the sub-cadences inside collectors (`kelvo_collect::Every`) fire when `continuous_ns` has moved at least the period since the last sample, with a jitter allowance of `min(period, interval) / 4`. Disk capacity is 60 s, load average and temperatures 5 s, fans 2 s, battery 10 s with slow fields at 60 s, and so on, at any interval. A period shorter than the interval fires every tick. Catalog `MetricDef::period_s` (was `cadence`) uses the same units.
- **Held-value staleness** is 2.5 times the largest of the catalog period, the owning collector's current period and the interval. It is computed each tick from when the value was sampled, so a collector that slows down (tray-only IOReport) does not blank its held values.
- **Storage.** S10 and M1 are unchanged. At 30 s and 60 s most S10 buckets have no sample; an empty bucket is never written and is never a gap (a gap is an absence of the sampler, not of a sample). M1 gets one row per minute at 60 s and per 30 s pair at 30 s.
- **Detail interest and tray-only mode.** The engine counts detail interest the way it counts process interest. The shell adds one unit per visible window with a live stream for that host; a hidden window or one with no stream adds nothing. With no detail interest (tray-only), IOReport (`Cadence::Adaptive { idle_ms: 10_000, interest: Detail }`) samples every 10 s instead of every tick.
- **Tray-only metric set.** The tray title and the hidden popover need `cpu.total`, `mem.pressure`/`mem.used`, `gpu.util`, `thermal.hottest`, `power.system`, `battery.*`, `net.rx/tx` and `disk.read/write`, which come from sysinfo, IOKit (GPU accelerator, battery, disk and network counters) and SMC/HID at their normal cadences. IOReport feeds `cpu.cluster.freq`, `cpu.cluster.active`, `gpu.freq`, `power.cpu`/`power.gpu`/`power.ane`/`power.dram` and cluster power from the `Energy Model`, `CPU Stats` and `GPU Stats` groups. In tray-only mode those M1 rows are fed every 10 s. IOReport values are counter deltas over the whole span since the previous sample, so the 10 s samples are exact averages and the M1 average is unchanged; only the M1 min/max envelope narrows.

### Consequences

The fix for "active" collectors found that a collector with no series keys (`Supported(empty)`, the process collector) was never sampled in the real app; it now runs when none of its modules is disabled. Collectors written later must use `Every`/`Cadence::Every` in milliseconds, never `tick.n % k`. A 0.5 s interval roughly doubles engine CPU; 30 s and 60 s cut it to the per-period collectors.

### Revisit when

Users want a non-listed interval, M1 min/max must be exact in tray-only mode, or a remote host needs detail interest over sync (v4).

## D-062: Engine performance gates: allocations, OS calls, store volume, CPU

Status: Accepted. Date: 2026-10-05. Phase 7 (performance).

### Decision

Four gates keep the engine light as it grows. Every threshold lives in `perf-budget.json` (section `engine`, next to the frontend gate of D-060) and nowhere else. **Raising one needs a new decision entry** that names the change that made the engine more expensive. Lowering one does not.

- **Allocations per tick** (`crates/kelvo-engine/tests/perf_gates.rs`, part of `cargo test`). A counting global allocator, the real macOS collectors and the engine on the fake ticker at 1 s, tray-only and window-open (process and detail interest). After 130 warm-up ticks it counts 120 ticks, attributing allocations per collector through a wrapper and leaving the rest to the engine core. Collectors default to a ceiling of 0. The allocator sees Rust heap allocations only; CoreFoundation and IOKit allocate through `malloc` directly, so the IOReport, IOKit and HID paths are zero here without being proven zero.
- **OS calls per tick** by API family (IOKit, SMC, IOReport, HID, libproc, sysctl/Mach), from per-thread counters in `kelvo_collect::calls` behind the `call-counters` feature. Only the engine's dev-dependency turns the feature on, so release builds compile the counters out. libproc's ceiling is per listed process, because it scales with the machine.
- **Store write volume.** A 10-minute tray-only run on a real SQLite file, extrapolated to rows and payload bytes per hour.
- **CPU.** `make perf` (`scripts/perf.sh`) runs the release engine (`dump --perf`, real collectors and store, no UI) for 120 s tray-only at 1 s, with a 30 s interval run beside it for comparison. It measures `getrusage` user + system after a 15 s warm-up and fails over `engine.perf.trayOnlyCpuPct`. In CI it is an advisory step on the macOS job (`continue-on-error`), since hosted runners are VMs with few sensors and noisy neighbours.

Measured on the dev M3 Max (macOS 27, about 1,100 processes, load average 2 to 4):

| Gate | Measured | Ceiling |
|---|---|---|
| Engine core allocations per tick, tray-only / window | 7.2 / 9.0 | 10 / 12 |
| Collector allocations per tick | 0, except battery 0.30, disk capacity 0.17, processes 0.05 (new processes) | 0; battery and disk capacity 0.5, processes 1, self CPU 0.1 |
| Calls per tick, tray-only: IOKit, SMC, IOReport, HID, kernel | 4.2, 11.9, 0.1, 9.2, 7.0 | 6, 16, 0.2, 15, 9 |
| Calls per tick, window: IOReport | 1.0 | 1 |
| libproc calls per tick per process, tray-only / window | 0.34 / 2.45 | 0.6 / 3.5 |
| Store, tray-only | 774 rows/h, 0.80 MB/h payload (S10 360, M1 60, process snapshots 354), file growth 2.6 MB/h | 900 rows/h, 1.2 MB/h payload |
| Release engine CPU, tray-only 1 s / 30 s | 0.60 to 0.63% / 0.12% | 0.9% (1 s) |

### Fixes found by the gates

- Process rows copied the name and user strings every sample: 1,539 allocations per sample, every tick while the process table is open. `ProcessSample::name` and `user` are now `Arc<str>` shared with the collector's per-process cache: 0 per sample in steady state.
- The `self.cpu` collector built a `String` per process to test a name prefix (156 allocations per sample when launched from a terminal). It now compares bytes in place.
- `SampleBuf::take_processes` sizes the next batch like the last one instead of growing from empty.
- Totals: 177 to 7.7 allocations per tick tray-only, 1,563 to 9.5 window-open.

### Before and after (D-061 cadence plus these fixes)

Release engine, four runs in parallel for 120 s, twice (the "before" binary predates D-061):

| Run | Before | After |
|---|---|---|
| Tray-only, 1 s | 0.96%, 0.99% | 0.61%, 0.63% |
| Window open (detail only) | 0.99% (IOReport always every tick) | 1.07% |
| Window open (detail + processes) | 0.95% (processes were never sampled, D-061) | 2.13%, 2.16% |

Tray-only drops by about a third: IOReport every 10 s saves about 0.45 points, and processes every 10 s (which before D-061 never ran) cost about 0.11. With a window open, sampling processes every tick costs about 1.1 points, nearly all system time in about 2,600 libproc calls per second (`proc_pidinfo` BSD and task info plus `proc_pid_rusage` for every process). That is the largest window-open cost and the next thing to cut, for example by reading thread counts less often.

### Consequences

A collector that starts allocating per tick, an IOReport or HID read that moves to every tick, or a new store row type each fails `cargo test` on macOS. The CPU gate is measured, not modelled, so it depends on load; its ceiling has about 45% headroom over the measurement. The libproc and HID ceilings depend on the machine's process and sensor counts; a machine with more of either may need a decision entry rather than a silent bump.

### Revisit when

The process table gets a cheaper sampling path, the app's idle budget is measured end to end with the WebKit helpers, or the tests move to self-hosted Apple Silicon runners where the CPU gate could be blocking.

## D-063: Live ring as typed series columns; streaming charts still rebuild their path each tick

Status: Accepted. Date: 2026-10-05. Phase 7 (performance).

### Decision

The frontend's live state keeps the last hour of rows twice: the existing row ring (for readers that walk whole rows: read-failure, overhead, the Timeline's live tail) and `SeriesColumns`, a circular `Float64Array` of timestamps plus one `Float32Array` per series key with `NaN` where a row did not measure the series. Appending a frame writes each column once, so a tick costs O(series). `seriesWindow`, `seriesStats` and `bucketAverages` read the columns instead of looking each key up in each row's layout map. Bucketed live charts (15m and 1h past 1,200 points) compute closed buckets once per bucket close and only the open bucket each tick. Float32 is enough for chart and window-stat values (seven significant digits); readouts still come from `held`, which is untouched.

`StreamArea` still rebuilds each series' path every tick, then slides it with the existing `translateX` (`useTickScroll`). architecture.md "Charts" describes appending one segment and translating; that is not done. On the CPU page with the 1 h window the profile puts d3-shape path building at about 0.8 ms/s of 18 to 23 ms/s, and the 60 s popover charts lower still. Re-anchoring an appended path, gap edges and right-aligned short series is more code than that saves today.

The per-core heatmap was the largest cost on the CPU page (about 10 ms/s of element creation). Its closed columns are memoized per row and its cells keyed by bucket number, so a tick re-renders one cell per row.

The perf gate (D-060) adds the CPU page on its 1 h window with an 800-process mock, budget `frontend.cpu1h` 35 ms/s (a new threshold, not a raise).

| Screen (perf gate, ms/s main thread) | Before | After |
|---|---|---|
| Popover | 19.2 to 23.0 | 13.9 to 21.1 |
| Overview | 19.1 to 20.5 | 14.8 to 19.0 |
| CPU page, 1 h window | 37.2 to 39.3 | 18.2 to 23.2 |

Before: HEAD before this change with the new CPU case, isolated runs. After: isolated and full-suite runs. M3 Max dev machine, dev-mode React in Chromium, as in D-060.

### Consequences

Each series costs 4 bytes per row of capacity (7,200 rows) in every window that streams, about 29 KB per series. The popover allocates the same capacity as the dashboard even though it only backfills 60 s.

### Revisit when

A chart shows path building above a few ms/s in the profile (a longer raw window, many series per chart), or the popover's memory footprint matters enough to size its columns to its window.

## D-064: Store and engine hardening from the architecture review: one writer, clock steps, peer identity, gated row kinds, typed store errors

Status: Accepted, amended by D-070 (clock-step hold capped at an hour; lost gap opens recovered). Date: 2026-10-05. Amends D-041. Architecture review fixes.

### Context

The architecture review found places where the one-way doors in architecture.md were held by convention only, or not at all: nothing stopped a second process from writing the history file; a wall-clock step back made the engine upsert into minutes it had already written; `is_local` travelled in the proto `Hello`, so a peer could claim to be the controller's own machine; a `SyncPage` row kind added later would be dropped silently by an older receiver whose cursor then moved past it; a store that opened but could not take the host row made the app exit at launch; and every store failure reached the webview as one untyped `store` error.

### Decision

**One writer per file (architecture.md infra 8).**

- `Store::open` takes an exclusive, non-blocking `flock` on `<file>.lock` before it touches SQLite and holds it until the writer has stopped. A second opener gets `StoreError::Locked`.
- The app registers `tauri-plugin-single-instance` (2.5.2) first, so a second launch shows the running instance's dashboard and exits before opening anything.
- Debug builds override the bundle identifier to `com.tryopendata.kelvo.dev` (`Context::config_mut` before build). They get their own data directory and their own single-instance socket, so `tauri dev` never shares the installed app's history.
- `kelvo_store::move_aside` renames a database and its `-wal`/`-shm` to `history-reset-<ms>.sqlite` while holding the same lock.

**Wall-clock steps (engine).**

- On each tick the engine compares the wall-clock delta with the continuous-clock delta. A difference larger than 2 base ticks, or a tick that is not after the previous one, counts as a step and is treated like a sleep:
  - the open buckets are flushed, then dropped (`TierAcc::reset`);
  - stall detection is skipped for that tick;
  - held-value timestamps shift by the step;
  - a new `layout_no` with the same series goes on the bus. That is the reset signal subscribers already handle.
- **Forward step.** The skipped span gets a `clock_changed` gap.
- **Backward step.**
  - Nothing is persisted until the clock passes the end of the newest bucket already written (`persist_from`), so an older bucket is never upserted with a second timeline.
  - The span from the stepped time to that point gets a `clock_changed` gap.
  - The ring buffer is cleared, because it must stay in time order.
- A step during sleep (the wake reading disagrees with the continuous clock) is handled the same way after the sleep gap closes.
- `GapReason` gains `clock_changed` and `write_failed`. Older readers see them as `unknown`, which D-040 already handles, and the frontend's gap switches have default arms.

**Peer identity (architecture.md infra 2 and 3).**

- `Hello.host` is a `HostIdentity { id, display_name, info }`.
- `is_local` exists only in the controller's `hosts` table and in `HostRecord` (store and IPC). `HostRecord` is a `HostIdentity` plus `is_local`.
- Schema v2:
  - adds `CREATE UNIQUE INDEX hosts_one_local ON hosts(is_local) WHERE is_local = 1`;
  - demotes all but the newest local host in existing files;
  - adds `cursors.kinds`.
- `upsert_host` of a local host demotes any other local host (a new install identity wins) rather than failing.
- `identity.rs` recovers a lost `host-id` file from `Reader::local_host()`, which the index makes unambiguous.
- Proto fixtures were regenerated. The skew test now sends an unknown field inside `host`.

**Row kinds are negotiated (amends D-041).**

- Each `SyncPage` row kind is a feature: `rows.buckets`, `rows.gaps`, `rows.events` (`SyncRowKind`, `SyncKinds` in kelvo-schema). A kind added later gets its own feature.
- The sender reads only the kinds both sides listed (`Negotiated::sync_kinds`). The receiver stores those kinds with its cursor.
- `resume_cursor` returns `None` (full resync) when the stored kinds do not cover the kinds negotiated now. That cursor moved past rows of a kind the receiver could not take then, and ingest is idempotent, so replaying costs bandwidth only.
- This was chosen over the alternatives (a cursor per kind, or a version-gated page shape) as the simplest design that is correct. Cursors stay one per `(host, tier)`, and the page shape is unchanged.
- v1 builds offer all three kinds (`kelvo_proto::local_features`).
- Test: `row_kinds_travel_only_when_negotiated` in kelvo-store's `tests/sync.rs`. An old receiver negotiates buckets and gaps, gets no events, and its cursor passes the event. After an upgrade it resyncs and the event arrives.

**Store failures never stop the app; errors are typed.**

- The shell registers the host itself (`History::register_host`). If that fails, it closes the store, marks history unavailable with the reason, and starts the source live-only. `LocalSource::start` no longer writes the host row.
- `CommandError` gains:
  - `store_busy` (SQLite busy or locked, and `Locked`);
  - `store_corrupt` (`Corrupt`, `Cbor`, `SQLITE_CORRUPT`/`NOTADB`);
  - `store_too_new`;
  - `internal` (a command task that panicked).
- `history_unavailable` carries an optional `reason`: `locked`, `too_new`, `corrupt` or `failed`. It serializes as `null` when unknown. tauri-specta's unified mode cannot express a skipped field.
- `store` stays as the fallback, because the frontend's banner matches it today.
- `reset_history` detaches every source from the store (flush, close its gaps), moves the file aside, opens a fresh one, registers the hosts and reattaches the sources (`SourceControl::set_store`, acknowledged by the engine). It fails with `store_busy` while another process holds the lock.

**Long work never blocks a flush.**

- Pruning and process roll-down handle the writer's queue between batches. Flushes, the engine's writes and upserts run in order. Another prune or a clear waits until the current one ends.
- A queued shutdown ends the prune early with `WriterGone`, and the writer then shuts down.

**A failed commit leaves a gap.**

- The writer tracks the wall-clock span of the buckets and process snapshots in its open batch.
- When `COMMIT` fails, that span is remembered. The next commit that succeeds writes a `write_failed` gap over it, so charts show a gap there instead of drawing a line.

**Per-host shell state.**

- `AppState::set_paused(host, paused)` pauses one host. The `set_paused` command and the tray target the local host, and the command's signature is unchanged.
- Engine settings apply only to the local host. They describe this Mac's sampler, and remote hosts get per-host overrides in v4.
- A `hosts-changed` event carries every `HostRecord` when one changes. Today the only change is the local host learning `chip_known`.
- `LiveFeed::now_ms` still reads the controller's clock. The live-channel rework replaces it.

### Consequences

- A clock step back loses up to the rest of the current minute of persisted history, plus the replayed span, which already exists from the first pass. Live values keep flowing throughout.
- A receiver that widens its row kinds pays one full resync per tier.
- `history_unavailable` now always carries `reason` in JSON, `null` when unknown.
- The new error kinds fall through the frontend banner's `store` match until the banner learns them; they still arrive as typed errors.
- Debug builds start with an empty history and their own settings and host id.

### Revisit when

- A third writer appears (the v4 agent shares the lock).
- A row kind needs ordering against another kind, which a per-kind cursor would handle.
- tauri-specta gains phased output for optional fields, so `reason` can be omitted instead of sent as `null`.
- Clock steps turn out to be common enough that losing the rest of a minute matters.

---

## D-065: Quit's OS side moves to kelvo-collect; the appstore feature reaches the app; CPU power calibration persists per chip

Status: Accepted. Date: 2026-10-05. Amends D-029, D-044, D-045 and D-054. Architecture review fixes.

### Context

The architecture review raised three problems.

- **Duplicated start-time formula.** The shell's `process_signal` had its own copy of `kelvo-collect`'s libproc reads: `bsd_info`, the start-time formula, `name_of` and the `responsible_pid` lookup. It was a copy because the shell does not depend on `kelvo-collect`. The UI sends back the `start_time_us` of the row the user picked, and the shell refuses with `pid_reused` unless its own read matches. If the two formulas drifted, every Quit would fail that way.
- **The `appstore` feature only reached `kelvo-collect`.** Neither `kelvo-engine` nor `src-tauri` had the feature, so no app build could select it. The shell also made calls a sandboxed build cannot or should not make: Quit and Force Quit signal processes outside the sandbox, and the responsible-PID lookup `dlsym`s an undocumented libSystem export, which App Review guideline 2.5.1 forbids. That lookup was also in `kelvo-collect`, used by `self.cpu`, a collector that declares `Entitlement::None`.
- **Calibration lost on restart.** D-054's calibration started every session at 1.0. In the first hardware run the PMP counters moved once in 45 minutes, so `power.cpu` could read about 25% low for over half an hour after each launch.

### Decision

**Quit's OS side lives in `kelvo-collect`.**

- `kelvo_collect::process_control` holds the `ProcessOs` trait, `ProcessInfo`, `OsError` and `StopKind`, plus `SystemProcessOs`. On macOS that is `MacProcessOs`; elsewhere a stub answers `Unavailable`.
- `MacProcessOs` reads name, start time and responsible PID through the same `libproc` helpers as the processes collector. It sends through `NSRunningApplication` and `kill(2)`, as before.
- `kelvo-engine` re-exports the module, so the shell gets it without a new dependency edge.
- The shell keeps the guard (`signal_process`: refuse list, start-time check, error mapping) and its specta types. The copied code is deleted.
- `kelvo-collect` now depends on `objc2-app-kit` (`NSRunningApplication` only). It is already in the graph through the shell and links only AppKit, a system framework.
- `info_start_time_matches_the_processes_collector` samples the real processes collector and checks that the test process's row has the same `start_time_us` and name as `MacProcessOs::info`. Changing either side's formula makes it fail (checked by mutation).

**The `appstore` edition is selectable and checked.**

- Features forward: `kelvo` `appstore = ["kelvo-engine/appstore"]`, and `kelvo-engine` `appstore = ["kelvo-collect/appstore"]`.
- `kelvo_collect::process_control::signals_available()` is false in the appstore edition and off macOS. There `MacProcessOs` answers `OsError::Unavailable`. `process_signal` checks it first and returns `ProcessSignalError::Unavailable` (`{ kind: "unavailable" }`).
- A new command, `get_edition`, returns `Edition { process_signal: bool }`. Capabilities describe hosts and their collectors, not what this app build can do, so the UI learns app-level gaps from here. No UI code reads the build feature.
- The responsible-PID lookup is compiled out under `appstore`: `responsible_fn` returns `None`. So in that edition `self.cpu` counts the app's own process without its WebKit helpers, and nothing else uses the lookup there.
- `make appstore-check` runs `cargo check -p kelvo --features appstore --all-targets`, then clippy over `kelvo`, `kelvo-engine` and `kelvo-collect` with the feature. `make check` and the macOS CI job run it. The edition is not run sandboxed or signed until v3.2.

**The rest of the shell, checked for sandbox safety.** None of it is private API, and none of it was changed.

- `SMAppService.mainApp` is the supported login-item API for sandboxed apps.
- `SCDynamicStoreCopyComputerName`, `sysctlbyname` and the `NSWorkspace` Reduce Transparency read are public. Reading them is allowed in the sandbox (unverified under an actual sandbox).
- `tauri-nspanel` and `window-vibrancy` use public AppKit classes and the public Objective-C runtime.
- One real sandbox problem: `tauri-plugin-single-instance` 2.5.2 listens on `/tmp/<identifier>_si.sock`. A sandboxed app cannot create that, so the appstore edition will need another single-instance path in v3.2. It is not a private API, so it is not gated now.

**Calibration persists per chip.**

- `kelvo_collect::calib::ScaleStore` (`load(chip)`, `save(chip, scale)`) is how `kelvo-collect` persists without writing files. The shell implements it as `FileScaleStore`, writing `power-calibration.json` in the app data directory (`~/Library/Application Support/<identifier>/`). The file is a map from the CPU brand string to the smoothed scale. Each save writes a temp file and renames it over the old one. A missing or corrupt file starts empty.
- The file is kept separate from `settings.json` because the scale is not a setting: no window reads or edits it, it changes without the user doing anything, and losing it costs only one session's seed.
- `EngineParts::platform(scales)` and `kelvo_collect::platform_collectors(scales)` pass the store to `CpuPowerSource`.
- When the SMC probe finds a chip's power map, it seeds the calibrator with the stored scale for that brand. If there is none, or the stored value is outside the 0.5 to 2.0 clamp, it uses the map's `default_scale`: 1.33 for the M3 Max, which is 1 / 0.75 from D-054's windows. A seed never replaces a scale learned in this session.
- Each window that closes saves the new smoothed scale. That happens on the engine thread after the calibrator lock is released, at most about once a minute and usually every 5 minutes or less often.
- The first window of a session smooths from the seed (weight 0.5), as any later window does.
- `power.cpu_source` gains 3, "seeded": scaled by a stored or default scale while no window has closed this session. 1 (uncalibrated) remains for a chip with a map but no default and no stored scale. 2 still means a window closed this session. The value stays an enum gauge, so a UI that does not know 3 sees an unknown value, not a type error.

### Consequences

- On the M3 Max, `power.cpu` is scaled from the first tick of every session, by 1.33 on first launch and by the last learned scale after that. Calibrated accuracy on hardware is still unverified (D-054). If the stored scale is wrong, it stays wrong until a window closes, and is then corrected only halfway per window.
- `kelvo-collect` now holds an action, not only reads. The `Collector` trait is untouched (D-044). `process_control` is a separate module that no collector uses.
- The appstore edition loses Quit and Force Quit and `self.cpu`'s helper attribution, in addition to the collectors v3.2's table lists.
- Frontend API changes: `ProcessSignalError` gains `{ kind: "unavailable" }`, which `signalOutcome` already maps to a toast. The new `commands.getEdition()` returns `{ process_signal: boolean }`. The transport does not expose it yet, and the Processes page should hide the actions when it is false. `power.cpu_source` can be 3.

### Revisit when

- v3.2 builds and runs the sandboxed edition: check what survives against this list and replace the single-instance socket.
- A second chip is verified: add its `default_scale` to its `CpuPowerMap`.
- The v4 agent needs process actions on a remote host. That needs its own decision (D-029).

---

## D-066: The live channel per host: a hub that owns the ring, projected channels, paced process rows, chunked backfill

Status: Accepted. Date: 2026-10-05. Amends D-047, D-049 and D-061. Architecture review fixes (#4 consumer side, #5, #6, #7, #12, #15, #25, #26, #28).

### Context

The architecture review found several problems with the live channel.

- **Process rows (#5).** Every window with process interest got every readable process (about 780 rows) at every tick, even the Overview, which shows five. While any window wanted rows, the collector re-read the 340 or so pids it can never read, and it read each process's thread count every second.
- **Backfill (#6).** `subscribe_live` was a sync command. An hour of ring history (3,600 rows of every series) was serialized on the main thread before the dashboard drew anything.
- **Layout (#7).** Every channel carried every series, including to windows that draw two of them.
- **Where the ring lived (#12).** The ring buffer, backfill and latest frame were inside the local engine (`Shared`, `SourceControl::backfill`). A v4 remote source would have needed its own copy. The shell also ran a second watcher that kept a latest frame of its own.
- **Stale interest (#15).** Process interest survived a page reload: the old page's interest stayed until the window closed.
- **Lag (#25).** A stream that fell behind the bus dropped the lost frames silently.
- **Unknown windows (#26).** A window label the window code never reported counted as visible, so a window it forgot to report streamed unseen.
- **Wall-clock tests (#28).** The live tests slept on the wall clock.
- **Clock step back (#4, consumer side).** After a step back the stream skipped every frame until the clock caught up with what it had sent.
- **`now_ms`.** `LiveFeed::now_ms` read the controller's clock, which is wrong for a remote source.

### Decision

**A `LiveHub` per host (kelvo-engine `live.rs`).**

- The hub wraps the host's `Bus` and retains the one-hour ring, the latest frame, the latest layout and the latest status. `publish` records first and then puts the message on the bus. So a consumer that subscribes and then reads the ring misses nothing: what was published before the read is in the ring, and what comes after reaches its subscriber.
- A `Source` publishes through `SourceSink.live`. The local engine no longer holds a ring, layout or latest frame (`Shared` keeps host, caps, status and interests). `SourceControl` loses `backfill`. A v4 `RemoteSource` feeds the same hub and gets backfill for free.
- The ring clears itself on a non-increasing timestamp, so any source's clock step back keeps it in time order. Before, the engine cleared it.
- The shell's `HostEntry` holds the hub. Its own latest-frame watcher is gone.
- Bus `LiveFrame` gains `interval_ms`, which the ring stores per row.

**`subscribe_live(host, channel, backfill_ms?, series?, min_period_ms?)`, async.**

- It runs on a runtime worker, not the main thread. Before returning it sends `Caps`, `Status`, a `Layout` for every layout the requested history uses, and the most recent two minutes (`RECENT_MS`) as `Backfill`.
- Older history follows as `backfill_earlier` chunks of at most 600 rows, newest first, after the first frame is out (or after 1 s if no frame comes). Each chunk is older than everything sent before it.
- `SubscriptionInfo` gains:
  - `stream`, the channel id;
  - `earlier_start_ms` and `earlier_rows`, describing what will still arrive.
- `series: SeriesSelector[]` projects the channel in Rust before serialization. `Layout.series`, backfill rows and frames carry the matching series only, in layout order. The projection is cached per layout.
- `min_period_ms` sends at most one frame per period, with half an interval of tolerance for jitter. A new layout always passes.
- Frames now carry `held` (the D-047 latest-value cache) next to `values`.
- "Now" for the backfill window is the hub's latest frame time, on the source's own clock. `LiveFeed::now_ms` is gone, so a remote source's history is measured on that host's clock.

**Process rows are shaped and paced in Rust (#5).**

- `set_process_interest(host, interested, view?, stream?)`. `view` is `ProcessView { limit, sort[], period_ms }`:
  - With `limit`, the batch is the union of the top `limit` rows for each sort key (cpu, memory, threads, wakeups, energy, disk_read, disk_write, disk_total), deduplicated by `(pid, start)` and sorted by the first key. Only picked rows are converted.
  - `limit: null` is the full table.
  - `period_ms` sends at most one batch per period, with 10% slack.
- The shell takes the shortest period over visible windows with active interest and gives it to the engine as `set_process_interest(Option<u32>)`. The engine turns process interest on only on the ticks that period is due (`Every::due_with`), so the collector itself slows down. The Overview (`{limit: 5, period_ms: 5000}`) no longer makes it run every tick.
- The collector skips pids whose `bsd_info` or `rusage` failed. It retries a pid after 60 s, and forgets it once it leaves the listing.
  - The cache is keyed by pid alone, not `(pid, start)`, because the start time is exactly what cannot be read without `bsd_info`.
  - So a pid reused between two listings by one of the user's own processes appears up to 60 s late.
- The collector reads thread counts every 5 s per process.

**Interest belongs to a page load (#15).** `stream` is `SubscriptionInfo.stream`.

- Interest with a stream counts only while that stream is the window's current one for the host.
- A new `subscribe_live` with a different id drops interest tagged with another id.
- Interest sent before its page's subscribe waits for it.
- Interest without `stream` keeps the old per-window behavior.

**Smaller fixes.**

- **#26.** Unknown window labels are hidden until `window_visible(label, true)`. The window code already sets this on show for the dashboard, popover and onboarding.
- **#25.** The bus reports lag (`Recv::Lagged`). The stream then catches up from the ring with exactly the span it missed.
- **#4.** A frame with a new `layout_no` and an older time than the channel has sent restarts the stream's timeline: sent span, frame pacing and process pacing all reset. The frontend should drop what it drew and start over from that frame.
- **#28.** The live registry tests run on a paused tokio clock, current-thread. Streams are tokio tasks.

**Proto (additive, skew tests pass).**

- `LiveFrame.held`, `#[serde(default)]`, omitted when empty, so v1.0 frames are byte-identical.
- `Message::LiveProcesses { ts_ms, rows: WireProcess[] }`, where every `WireProcess` field has a default. An older receiver decodes it as `Unknown`.
- New fixtures `live_held.cbor` and `live_processes.cbor`. The existing fixtures are unchanged.

### Measurements

Release `dump` engine, window open (`--interest processes,detail`). The 0b7dca0 build and this one ran in parallel, 120 s at 1 s, twice.

- **libproc calls per tick** (perf_gates, about 1,135 pids):
  - with window interest, 2,691 before and 1,720 after (2.47 and 1.62 per process);
  - tray-only, 269 before and 239 after.
- **Engine CPU, full table at every tick:**
  - run 1: 1.13% before, 1.13% after;
  - run 2 (a busier machine): 1.86% before, 1.87% after.
  - So the failed calls and thread reads the collector skips were cheap; the cost is in the readable processes' `bsd_info`/`rusage` and the listing.
- **Engine CPU, Overview-style 5 s period** (`processes=5000,detail`): 0.72% in run 1 and 1.17% in run 2, about 37% below the full table in each run.

### Consequences

- A subscriber sees history in two phases. The current frontend ignores `backfill_earlier` (unknown kind), so until it prepends those chunks the dashboard shows two minutes of ring history, not an hour.
- The frontend must reset its live state on a new `layout_no` whose frame is older than what it holds. Today's reducer drops such frames as not newer.
- A window with several process consumers must send the union of their views. A new call replaces the window's view.
- The process table on the Processes page costs what it did. Only narrower views got cheaper.

### Revisit when

- v4's `RemoteSource` lands: it publishes `LiveProcesses` and `held` into its host's hub.
- Process sampling CPU matters again: the remaining cost is per readable process, so a cheaper call or a lower full-table rate is the next lever.

## D-067: Idle CPU is measured end to end; collectors not on screen sample every 10 s; CPU gates get a fixed target and a baseline ratchet

Status: Accepted, amended by D-070 (only counter-derived collectors idle at 10 s). Date: 2026-10-05. Phase 7 (performance).

### Context

The architecture review found that nothing measured the budget the product promises: under 0.5% idle CPU for the app plus its WebKit helpers, and at or below Stats. The specific gaps:

- `bench-vs-stats.sh` did not exist.
- The only CPU gate measured the engine alone, and its 0.9% ceiling let usage drift to 1.8 times the whole-app target.
- `perf_gates` passed with nothing measured when the hardware collectors probed Unsupported.

### Decision

**Measure the whole app.**

- `scripts/bench-coalition.sh` (`make bench`) builds the packaged app with the `bench` Cargo feature under its own identifier (`com.tryopendata.kelvo.bench`, so the user's settings and history are untouched).
- `KELVO_BENCH_SCENARIO` (`tray`, `popover`, `dashboard:<route>`) opens a window 3 s after launch. `src-tauri/src/bench.rs` reads it only with the feature on; shipped builds ignore it.
- `crates/kelvo-collect/examples/coalition.rs` sums `proc_pid_rusage` CPU and `phys_footprint` over the app and every process macOS holds it responsible for, using `self.cpu`'s membership rule. It also reports the app's CPU per thread.
- `scripts/bench-vs-stats.sh` (`make bench-vs-stats`) runs Kelvo and Stats tray-only for 10 minutes each. It skips with a message when Stats is not installed.
- `dump --perf` reports each collector's thread CPU time (`CLOCK_THREAD_CPUTIME_ID` around each sample), the engine core, and the process's threads. `make perf` prints the same breakdown.

**Budgets: a fixed target plus a baseline ratchet** (`perf-budget.json`).

- `coalition.target.trayOnlyCpuPct` is 0.5, the product budget. Changing it is a user decision, not a ratchet.
- `coalition.baseline` and `engine.perf.baseline` hold the last measurement. A run more than `regressionPct` (10%) over its baseline is measured again, and fails if the second run is also over.
- Every run prints its distance to the target. `make perf` blocks locally and stays advisory on hosted CI, where it prints the engine's share of the target as a notice. `make bench` needs a real menu bar and a hands-off machine, so it runs locally only.

**Coverage in `perf_gates`.**

- Every collector that probed `Supported` must take at least one sample per its slowest period.
- On real Apple Silicon, `ioreport`, `smc.power`, `smc.sensors` and `hid.thermal` must probe `Supported`.
- On a VM (`kern.hv_vmm_present`), a non-Apple-Silicon host or the appstore edition, the test prints `[perf] skipped: no hardware (...)`. CI runs perf_gates in its own step and turns that line into a notice.

**Interest-driven cadence for everything on screen** (generalizing D-061).

- New `Interest::Live` and `Cadence` constant `LIVE_OR_IDLE`: every tick while any visible window streams (detail interest), or while the menu bar draws a value from the collector's module (`Settings::menu_bar_shows`); otherwise every `IDLE_MS` (10 s).
- `cpu`, `disk_io` and `network` use it (D-070; as first written, `gpu`, `memory` and `smc.power` did too). With the default menu bar (CPU, GPU and Memory bars plus the hottest temperature), network and disk I/O drop to every 10 s when no window is open.
- Their rates and the M1 averages built from them stay exact, because each is derived from cumulative counters: the 10 s delta covers the whole interval.
- `gpu` (Device Utilization %), `memory` and `smc.power` (`PSTR`, CPU and cluster watts) are instantaneous gauges, so they stay on `EveryTick` (D-070). Sampled every 10 s, their M1 averages would be the mean of six points. That also keeps `smc.power`'s calibration windows running tray-only.
- Between samples the held value (2.5 times the period) keeps the menu bar and S10 rows filled.

**`self.cpu` caches coalition membership.**

- A pid's responsible-PID lookup is asked once, and its rusage start time is checked each sample, so a reused pid is asked again.
- Everything is asked again every 5 minutes.
- On about 1,100 processes this took `self_cpu` from 0.034% to 0.001%.

### Measurements

Dev M3 Max, macOS 27, about 1,130 processes, load average 2.4 to 3.4. Absolute numbers on this machine swing up to 2 times with load, so before and after ran side by side.

**Release engine, tray-only at 1 s.** Parallel `dump --perf` runs of HEAD and this change, 120 s each, twice:

| Run | Before | After |
|---|---|---|
| 1 | 0.509% | 0.378% |
| 2 | 0.402% | 0.250% |
| `make perf`, later, alone | | 0.329% |

Where the engine's time goes after the change (the `make perf` run):

| Collector | CPU |
|---|---|
| processes (every 10 s, about 1,130 pids) | 0.089% |
| hid.thermal (every 5 s) | 0.058% |
| ioreport (every 10 s) | 0.037% |
| smc.sensors (every 5 s) | 0.024% |
| gpu | 0.022% |
| battery | 0.013% |
| cpu | 0.010% |
| smc.fans | 0.010% |
| memory | 0.007% |
| network, self_cpu, disk_io, smc.power | 0.003% to 0.007% each |
| engine core (ring, rollups, bus, store hand-off) | 0.028% |

`smc.power` was sampled every 10 s in this run; D-070 puts it back on every tick, and its cost there is in D-070. `gpu` and `memory` ran every tick here already, because the default menu bar shows them.

The 30 s interval costs 0.085%.

**Whole app, tray-only, default menu bar.** The "before" bundle is HEAD's cadence with the same frontend, alternated with "after", 120 s each after a 30 s warm-up:

| Bundle | Coalition | App main thread | kelvo-engine thread | WebKit helpers |
|---|---|---|---|---|
| Before | 1.433%, 1.391% | 0.897%, 0.875% | 0.433%, 0.417% | 0.002% |
| After | 1.366% | 0.893% | 0.358% | 0.003% |

Footprint is about 101 MB across 6 processes. The hidden webview's WebContent is 38 MB, and the app itself 34 MB.

- **The tray is the cost now.** The main thread draws the menu bar image every tick. That is about 0.9 points, plus 0.06 in GCD threads and 0.02 in the tray thread.
- With no menu bar items configured, the coalition measured 0.41% in an earlier run.
- **The WebKit helpers are idle while hidden** (0.002%).
- A run with a window accidentally open measured 7.3% and 579 MB across 8 processes: WebContent 3.0%, app 2.7%, WebKit GPU 1.5%. It is not a scenario result, but it shows the order of window-open cost.

### What is left to reach 0.5% tray-only at 1 s

The engine change was worth about 0.07 to 0.15 points. The coalition is still about 0.9 points over the target, and most of that is drawing the tray, not collecting. The options, none of them taken here:

1. **Cheaper tray redraws** (`src-tauri/src/tray/`).
   - Skip the redraw when the rendered image is unchanged at menu bar resolution.
   - Drop tray-icon's PNG encode and decode round trip.
   - Avoid the status item's relayout on every set. In earlier profiling this was about 27% of the main thread's busy samples.
   - This keeps the product as it is and is the largest lever.
2. **Redraw the tray at most every 2 s while sampling at 1 s.** About half the tray cost; the menu bar updates at half the rate.
3. **A 2 s default interval.** Roughly halves both the tray and the engine. This changes a user-visible default.
4. **Engine leftovers.**
   - processes every 30 s tray-only, which lowers `proc_snap` resolution in the store.
   - hid.thermal and smc.sensors every 10 s when the menu bar shows no temperature.
   - Together under 0.1 point.

Options 2 and 3 change what the user sees, and moving the 0.5% target is a user decision. The baseline is set at today's 1.41% so that nothing gets worse while that is decided.

### Consequences

- Live-interest collectors (`cpu`, `disk_io`, `network` since D-070) sample every 10 s with no window open, unless the menu bar shows their module. Their S10 rows then hold one sample instead of ten. M1 averages are unchanged, because they come from counters.
- A new collector reading a counter should use `LIVE_OR_IDLE` unless something on screen needs it every tick.
- `perf_gates` fails on an Apple Silicon Mac where a private-API collector stops probing Supported, for example after a macOS change. That is intended: the alternative is a gate that measured nothing.
- The ratchet only catches regressions against the last measurement. The distance-to-target line is what shows the remaining gap.

### Revisit when

- The tray redraw work lands: lower `coalition.baseline` to its measurement.
- A self-hosted Apple Silicon runner exists: `make perf` and `make bench` can become blocking in CI.

---

## D-070: Gauges sample every tick; a backward clock step holds history for at most an hour; lost gap opens are recovered; perf and prune tests measure what they claim

Status: Accepted. Date: 2026-10-05. Amends D-064 (clock steps, failed commits) and D-067 (idle cadence). Review fixes (#2, #7, #9, #14, #20). Amended by f9137e7: `discard_from` commits on its own and is retried, and pause, resume and module switches flush.

### Context

The review found five problems in the collector, store and engine crates:

- **#2.** D-067 put `gpu`, `memory` and `smc.power` on `LIVE_OR_IDLE` and said their M1 averages stay exact because all are counter-derived. They are not. GPU Device Utilization %, the memory figures and SMC `PSTR`, CPU and cluster watts are instantaneous gauges. Sampled every 10 s, a minute's average is the mean of six points.
- **#7.** After a backward clock step, `persist_from` held all persistence until the clock reached the newest bucket ever written. A clock that had been months ahead stopped history for months. The hold also survived `set_store` and was not visible anywhere.
- **#9.** A failed commit recorded `write_failed` over its buckets and process snapshots only. Gap opens and closes in the lost batch vanished. `close_gap` did nothing when no open row existed, so a sleep gap whose open was lost was never recorded.
- **#14.** `perf_gates` counted a sample even when `sample()` errored or pushed nothing, so a hardware collector that failed every sample still passed coverage.
- **#20.** The store and engine prune tests raced a 5,000-row batch prune against wall time.

### Decision

**Only counter-derived collectors idle (#2).**

- `cpu`, `network` and `disk_io` keep `LIVE_OR_IDLE`.
- `gpu`, `memory` and `smc.power` are back on `EveryTick`.
- `only_counter_collectors_idle_when_not_shown` in kelvo-collect lists the collectors whose cadence is `LIVE_OR_IDLE` and fails when a gauge joins them.
- `smc.power` on every tick feeds the calibrator's fast side each second again, so calibration windows run tray-only (IOReport's 10 s tray-only period is still under 5% of a 300 s PMP window). This is the pre-D-067 path.
- Cost: one release `dump --perf` run, tray-only at 1 s for 120 s, on a machine busy with other builds. `smc.power` measured 0.018% of a core every tick, against 0.003% to 0.007% at 10 s in D-067. `gpu` and `memory` were already every tick in that run, because the default menu bar shows them. The whole engine measured 0.15%.

**A backward clock step holds persistence for at most an hour (#7).**

- The hold ends at the newest bucket already written or one hour after the step, whichever comes first, rounded up to a minute boundary (`MAX_PERSIST_HOLD_MS`).
- When the old clock wrote past the hold, the engine queues `Writer::discard_from(host, hold_end)`. It deletes every row of that host stamped at or after the end of the hold: S10 and M1 buckets, process snapshots and roll-ups, events, and gaps that start there. A closed gap that spans the cut is cut at it. The new timeline therefore never upserts into the old clock's rows. The `clock_changed` gap covers the step to the end of the hold.
- Amended by f9137e7: `discard_from` is no longer queued. It is a replied call that commits the batch before it, then the discard in its own transaction. The engine keeps the cut pending and retries it on every tick past the hold until the store confirms it, and does not persist until then, so the hold can last longer than an hour while a discard is unconfirmed. After a confirmed discard the engine opens again the `module_disabled` and `paused` gaps it holds open, which the discard deleted with the old clock's rows, and commits them. It also writes the last step's `clock_changed` gap again, extended to the time of the confirmed discard, because nothing was persisted until then and the discard's cut (possibly an older step's) can delete or trim that gap. A later step while the discard is pending widens that remembered gap from the earlier step's start instead of replacing it, and keeps the hold even when it needs none of its own.
- `set_store` clears the hold and any pending discard. It protected the old file's buckets, and a new file has none.
- `EngineStatus::history_held_until` carries the hold. It is published when the hold starts, ends or is cleared. Surfacing it in `HistoryHealth` and the UI is shell and frontend work.

**A gap whose open was lost is still written (#9).**

- `Writer::close_gap` takes the gap's start as an optional argument. When no open row exists and a start is given, it writes the closed gap `[start, end]`.
- The engine tracks when its sleep and paused gaps started (`sleep_gap_start`, `paused_gap_start`) and passes that start with each close. Module-disabled gaps pass none.
- The writer's lost span now also widens over gap opens, closes and writes in the failed batch, so the `write_failed` gap covers them.

**Perf coverage counts what was measured (#14).** `Measured` counts errors and samples that pushed no value and no process row. On Apple Silicon hardware, a `HARDWARE_COLLECTORS` entry that errors or is empty in more than half its samples fails coverage. The `--nocapture` lines print both counts per collector.

**Prune batch size is configuration (#20).** `StoreConfig::prune_batch` (default `DEFAULT_PRUNE_BATCH`, 5,000) replaces the constant. The two prune tests use 7 rows per batch over 21,000 rows, so the prune is 3,000 batches long. They assert that the rows deleted when the flush or shutdown is handled are a whole number of batches. 7 does not divide 5,000, so the tests also fail if the configured size is ignored.

**History commits every 5 minutes (user decision).** `DEFAULT_COMMIT_INTERVAL` goes from 30 s to 300 s. The user weighs disk writes over the crash-loss window. The engine still flushes before sleep, on wake (so the closed sleep gap is readable at once), on a store swap and at shutdown. Amended by f9137e7: it also flushes on pause, on resume (after closing the pause gap, or opening a sleep gap when resumed while asleep), when a module is switched off or on, on a wake while paused (after the pause gap is opened again, not before), and after the gaps a discard deleted are opened again, so readers never draw a stale open `paused` or `module_disabled` gap over new samples; and `discard_from` commits itself. Uncommitted rows are invisible to readers, so the newest up to 5 minutes exist only in the live ring.

- **Measured.** `kelvo-store`'s `write-stats` feature (a dev-dependency feature of kelvo-engine, never in the app build) counts commits and the pages the write connection wrote to the WAL (`SQLITE_DBSTATUS_CACHE_WRITE`, which counts every WAL frame, cache spills included). The perf gate's store run flushes every commit interval of fake time, because the writer's own timer runs on wall time and the fake ticker outruns it. Real collectors, 170 series, tray-only at 1 s, one hour of fake time, dev M3 Max (`wal_volume_by_commit_interval`, ignored, by hand):

  | Commit interval | Commits/h | WAL frames/h | Bytes appended to the WAL/h |
  |---|---|---|---|
  | 30 s | 120 | 1,162 | 19.1 MB |
  | 300 s | 12 | 236 | 3.9 MB |

  - Each commit rewrites about 10 pages of 16 KiB (b-tree leaves and interiors, `meta.next_seq`) for about 2 KB of new payload at 30 s. At 300 s, that is about 20 pages for ten times the payload. Rows and payload are unchanged (774 rows, 0.80 MB/h).
  - Checkpoint copies into the main file are not counted. They are bounded by the distinct pages in each 256-frame checkpoint window, so they fall with the frame count.
  - The 10-minute gate (`store_write_volume_per_hour_at_one_second`) now prints commits and WAL bytes per hour at the default interval: 12 commits/h, 4.04 MB/h. `engine.store.walBytesPerHour` is asserted once the budget names it.
- **Frontend at the right edge.**
  - The Timeline memo also keys on the live edge and `rowsEpoch`, not `rowsVersion` (a rebuild every tick). Earlier ring chunks that land behind the newest row, and a backfill that arrives after the history page in the bucket the view was mounted in, now redraw the span history does not have yet.
  - `useProcessesAt` keeps an answer forever only when it was fetched more than the commit interval plus 30 s after its bucket ended. An earlier answer, often `null`, goes stale at once, so the cursor's next visit asks again.
  - The battery bars' running hour takes the current reading (`withCurrentReading`), and the hours refetch every commit interval, so the closed hour's last minutes arrive.
  - `HISTORY_COMMIT_MS` in `src/core/history-state.ts` mirrors the Rust constant by hand.
  - **Not changed:**
    - The Overview's 24 h maxima (GPU power and disk bar scales) ignore a spike in the uncommitted minutes. They were already up to 10 minutes behind through their own refetch interval. Folding in the ring would need a ring max on the 1 Hz Overview path.
    - "last wake" is unaffected by the commit interval, because the engine flushes on wake. Its 10-minute cache can still show the previous wake, as before.

### Consequences

- Tray-only engine CPU rises by about 0.01 to 0.015 points for `smc.power` (one run). Memory and GPU are unchanged with the default menu bar. Without CPU, GPU or Memory in the menu bar, `gpu` and `memory` now cost what they did before D-067.
- After the clock steps back more than an hour, history the wrong clock wrote ahead of the new time is deleted. If the new clock is the wrong one (someone set the date a year back), that deletes real history from the new "now" on. The engine trusts the current clock for everything it writes, so this was chosen over overwriting through upserts.
- `discard_from` does not move the `pruned` marks. A remote cursor past the deleted rows keeps whatever it already synced. In v1 no remote reads the local file.
- A lost close of a gap whose open committed still leaves that gap open until the next close or session start. That is unchanged.
- A crash or power loss loses up to 5 minutes of history, against 30 s before. WAL appends drop about 5 times.

### Revisit when

- The shell shows `history_held_until`.
- `query_history` merges the engine's ring for the uncommitted span. Every reader then sees the newest minutes, and the frontend workarounds above can go.
- A trustworthy time reference (for example the last NTP sync) exists to decide which clock is wrong before discarding.

---

## D-071: The host id is bound to the Mac; unusable data and log directories never stop the launch

Status: Accepted. Date: 2026-10-05. Amends D-064 (peer identity, store failures). Review fixes.

### Context

- `host-id` was a bare UUID. Migration Assistant, a disk clone or a restore to new hardware copies it, so two Macs ran with the same id. In v4 the controller's `ON CONFLICT(uuid)` upsert would then set `is_local = 0` on its own host and merge the other Mac's rows into it.
- Startup still exited when the log directory, the data directory or the `host-id` file could not be created, read or written: a data directory left root-owned by a `sudo` run, or a full disk on first launch. D-064 had already made a broken store survivable; these paths had not been.

### Decision

**Machine binding.**

- `host-id` gains a second line, `machine=<hex>`: SHA-256 (CommonCrypto, in libSystem) of a fixed prefix, the host UUID and `IOPlatformUUID` (public IOKit; expected to be readable in the App Sandbox, not yet checked in an `appstore` build, and a Mac where it cannot be read simply skips the check). Salting with the UUID keeps the hardware UUID out of the file and makes the token useless across installs.
- At launch the id (from the file, or recovered from the store's local host) is checked against this Mac's token. A mismatch makes a new id (`IdSource::Cloned`). The store's old local host is not reused either, since it came with the copy.
- A file without the line (or a Mac whose hardware UUID cannot be read) is trusted and the line is added.
- **The old history stays.** Registering the new local host demotes the copied one to a non-local host (D-064's newest-local-wins). Its rows are pruned by retention like any host's; v1 shows only the local host, so they are invisible until then. Deleting them was rejected: it destroys data on a misdetection, and the cost of keeping them is bounded by retention.
- **Not covered:** a copied store whose `host-id` file was lost too. Nothing records which Mac the store's local host belongs to, so it is reused. The two rules below are the backstop.

**The store refuses a remote host with the local id.** `upsert_host` of a non-local record whose UUID is the stored local host's fails with `StoreError::HostConflict` and changes nothing. The v4 handshake must reject a peer whose `HostIdentity.id` equals the controller's own local id before it reaches the store (architecture.md infra 2).

**Startup survives unusable directories.**

- Logging falls back to stderr when the log directory cannot be resolved or opened; the error is logged there.
- A data directory that cannot be created leaves history unavailable with the store's reason (live-only, the Settings banner explains), as a store that cannot open already did.
- A host id that cannot be read is recovered or made as if missing. One that cannot be written is used for this run only and logged. The next run recovers it from the store when history was written under it.
- What still fails the launch: an unresolvable app data directory (no home directory), the engine thread failing to start, and creating the tray, popover or onboarding window.

**Two smaller fixes from the same review.**

- `engine_affecting` compares `EngineSettings::from_settings` for the old and new settings. It had compared only sampling and module switches, so a menu-bar-only change never reached the engine, whose idle cadence depends on what the menu bar shows (D-067).
- `reset_history` emits `history-health-changed` only after the store is back. The frontend's health hook refetches instead of overwriting a cached `history_unavailable`, so a stray event cannot hide the banner. `history_health` is async and runs on a blocking thread like the other store commands, so a call during a reset no longer blocks the main thread on the store lock.

### Consequences

- A Mac set up from another Mac's backup starts a new host, with its own empty history, on its first launch after this change.
- `host-id` is no longer a single line. Anything that reads it should take the first line.
- A launch with an unwritable data directory runs, but shows no history and makes a new id each time until the directory is fixed.

### Revisit when

- v4 sync lands: the handshake check above is required, with a test.
- The store learns which Mac wrote it (for example a binding in a `meta` row), which would close the lost-file case.

## D-072: The live channel's clock steps are an explicit timeline; resumes go in chunks; display sleep and frame pacing reach the status

Status: Accepted. Date: 2026-10-05. Amends D-064 (clock steps, consumer side) and D-066. Review fixes #5, #6, #10, #11, #12, #13, #21.

### Context

- **#5.** A stream that resumed (window shown, display awake, a lagged subscriber) sent every ring row since the last one as one `Backfill`: up to an hour, about 3,000 rows of 170 series, which the webview applied in one task.
- **#6.** `subscribe` took the registry lock twice with the backfill in between. With two concurrent subscribes for one window and host, A then B, A's second lock replaced B's stream and aborted it. React StrictMode subscribes twice, so every dev load hit it.
- **#10, #12.** A clock step was inferred on both sides from "a new `layout_no` with a time not after what is held". A module toggle plus the reconnect overlap in `host-store` (two intervals) matched that and cleared the window's rows. The ring cleared itself on any non-increasing time, and so did the frontend, so at 0.5 s a 1.1 s NTP step back wiped the hour. The engine's step threshold was 2 ticks, 1 s at 0.5 s.
- **#11.** A window hidden across a step back resumed from `max(now - backfill, last sent + 1)` with the old timeline's last time: it got nothing, then skipped frames until the clock caught up.
- **#13.** `LiveStatus` did not carry `display_idle`, while the stream sends no frames during display sleep, so `host-store` went stale, resubscribed with backoff, cleared rows and asked for the hour again on each attempt. A stream paced by `min_period_ms` past three intervals would reconnect forever.
- **#21.** `select_processes` sorted with `partial_cmp().unwrap_or(Equal)`, not a total order once a value is NaN.

### Decision

**Timeline.** `LiveFrame` (kelvo-engine) and the IPC `Frame` and `Backfill` carry `timeline: u32`. The engine bumps it in `on_clock_step` and nowhere else; the layout bump stays but is no longer a signal. Ring rows keep their timeline and segments split on it.

- A consumer holding rows on another timeline drops the rows at or after the first row of the new one, and keeps the older ones. Nothing compares layout numbers with times any more, in Rust or TS.
- After a step the stream sends the new timeline from its first row in the ring (bounded by the window's backfill span), so that row's time is where the window cuts. This also covers #11: a resume whose "now" is before the last thing sent, or whose latest frame is on another timeline, starts the stream's timeline over first.
- `BackfillEarlier` carries no timeline: it is always older than what the window holds and never cuts.

**Ring truncation.** `Ring::push` with a time not after the newest drops only the rows at or after it. The frontend's `RowRing` and `SeriesColumns` gain `truncateFrom`. A 1.1 s step costs two rows, not the hour.

**Step threshold floor.** `clock_step_threshold = max(2 ticks, 2 s)`. A tick that is not after the previous one is still always a step (the ring must stay ordered).

**Resume in chunks, in time order.** A resume sends the missed span as `Backfill` messages of at most `EARLIER_CHUNK_ROWS` (600) rows, oldest first, all before the next frame. The review suggested reusing the fresh split (last two minutes as `Backfill`, the rest as `BackfillEarlier`). Rejected: `BackfillEarlier` goes in front of the oldest held row, while a resume's missed span sits between the held rows and the recent ones. That needs an insert into the middle of the circular column ring, and a clock step while hidden would need it to replace rows too. In-order chunks bound the per-message cost the same way, and need no new frontend semantics.

**Subscribe race.** Each `subscribe` takes a registry-wide sequence number at its first lock and records it on the slot. At its second lock it installs its stream only if the slot still has its number; otherwise it aborts its own task. The newest subscribe wins.

**Status.** `LiveStatus` gains `display_idle` and `frame_period_ms` (the base tick, or the multiple of it `min_period_ms` thins frames to, computed with the stream's own half-interval tolerance). `host-store` does not arm stale or reconnect while paused or display-idle, and measures stale in frame periods. The reducer clears stale on a display-idle status as it does on pause.

**Process sort.** `total_cmp`, with NaN mapped to negative infinity so it ranks last.

### Consequences

- The IPC types changed (`timeline` on `Frame` and `Backfill`, two `LiveStatus` fields); bindings regenerated. No wire (CBOR) type changed.
- The mock transport models visibility (`setWindowVisible`), display sleep (`setDisplayIdle`), `fastForward`, timelines and the chunked resume, delivered one message per task with frames queued behind it. The `hidden-resume` scenario drives a perf-gate case (`dashboard shown after half an hour hidden`) that reuses `dashboardOpen.longTaskMs` until it gets its own `perf-budget.json` entry.
- On the Chromium mock, one 1,801-row `Backfill` on the 170-series layout did not produce a task over 50 ms, so the long-task half of that case does not distinguish the old behaviour there; the WKWebView JSON parse of an hour of rows is the larger cost and is not measured by the mock. The case's subpath check fails when a frame is applied before the resumed rows (see below).
- A hole over the resumed span (1 h chart and per-core heatmap) appears only when a frame newer than the missed span is applied before the resume: `RowRing.push` and `SeriesColumns.push` drop rows not newer than the newest held (D-049), so the whole span is discarded. One unchunked `Backfill` delivered ahead of the next frame draws correctly; the chart caches key on the newest bucket and `rowsEpoch` and handle any append size and ring wrap (regression tests in `use-window-series.test.tsx`, commit 0105566). The ordering guarantee lives in the producer: resume chunks go oldest first and frames queue behind them. A dropped out-of-order span is silent today, with no log or counter.
- A step back smaller than the 2 s floor whose tick still lands after the previous one is not a step: up to 2 s of wall-clock disagreement is absorbed into the rows' times.

### Revisit when

- A new producer (v4 remote source) delivers live rows: it must keep resume rows ahead of newer frames, or the reducer should count rows it drops as out of order.
- The resume perf case gets its own budget entry.

## D-073: The tray swaps its image straight into a pinned-length status item

Status: Accepted. Date: 2026-10-05. Phase 7 (performance). Follows D-067 option 1.

### Context

D-067 left the whole app at about 1.37% tray-only against the 0.5% target, with the app's main thread (about 0.9 points then) mostly redrawing the menu bar image. Every drawn frame went through tray-icon's `set_icon_with_as_template`: RGBA to PNG on the main thread, `NSImage initWithData:` to decode it, `setImage:` with the image still at its pixel size (so the status item grew to 2x and then shrank on `setSize:`), `setImagePosition:` (a second `_adjustLength`), the template flag set after the image was already on the button, and the accessibility label in a second main-thread dispatch.

`sample` on the HEAD bench bundle, tray-only at 1 s, 90 s: the main thread was busy in about 1.2% of wall-clock samples. Our dispatch (PNG encode and decode, two `_adjustLength` relayouts) was about 23% of that; the rest was AppKit and Core Animation reacting to the new image: redrawing the button, the status item scene updates and fences, `_updateReplicants` snapshots and the display cycle.

The frame skip (`FrameSkip`) already covers the real path: the frame is quantized to what the icon shows (bars in device pixels, text as drawn), so equal frames render equal images. It skips 23 to 50% of ticks depending on how jumpy CPU is. Nothing reaches the main thread for a skipped frame.

### Decision

- A frame whose image keeps its pixel size goes through `platform::appkit::set_status_item_image`: the RGBA is copied into an `NSBitmapImageRep`, wrapped in an `NSImage` already at its point size (18 pt tall, width scaled as tray-icon does) and flagged as a template, and set on the button once. The accessibility label is set in the same main-thread call, and only when its words changed (`model::ItemState`).
- After a size change, the status item's length is pinned to the width AppKit laid out (`pin_status_item_length`), so a swap does not re-measure the button.
- A size change, or a failed swap, releases the length, goes through tray-icon as before (it also resizes its click target view), and pins the new width.

Same image bytes, same size, same template flag, same 1 s cadence. The bitmap is premultiplied RGBA with black color channels, which is identical to the straight-alpha PNG tray-icon made from it.

### Measurements

Dev M3 Max, macOS 27, while other agents were building in parallel (load average 4 to 115). Bench bundles built from clean exports of HEAD and of the change, with the same frontend build.

**Parallel runs** (HEAD as `com.tryopendata.kelvo.bench`, the change as `com.tryopendata.kelvo.bench2` with the same settings, both tray-only at 1 s, 120 s after 30 s warm-up, measured at the same time):

| Run | Coalition before | Coalition after | Main thread before | Main thread after | Frames drawn per minute (before / after) |
|---|---|---|---|---|---|
| 1 | 0.562% | 0.505% | 0.259% | 0.199% | 41 / 44 |
| 2 | 0.663% | 0.604% | 0.321% | 0.258% | 47 / 48 |
| 3 | 0.355% | 0.330% | 0.135% | 0.100% | 32 / 30 |

The main thread drops 20 to 26%, about 0.035 to 0.065 points; the coalition drops by the same amount. The engine thread is unchanged (0.19 to 0.28%).

**Alternating `make bench` runs** (one app at a time, 120 s each): before 1.036, 0.735, 1.141%; after 0.991, 0.806, 0.679%. Single runs swing by 0.4 points with load and with how many frames CPU jitter forces, so only the parallel runs resolve the change.

Runs with a dashboard window open (500 MB footprint, 7 to 10%) were discarded. During the session something set the bench identity's interval to 2 s (`settings.json`); it was put back to 1 s before the runs above. At 2 s, HEAD measured 0.54 to 0.77% and the change 0.56 to 0.93% in alternating runs.

**Per drawn frame** a redraw still costs the main thread about 5 to 8 ms of CPU, almost all of it in AppKit after `setImage:` (button redraw, scene update, replicant snapshot). That part does not depend on how the image is built.

### What is left to reach 0.5% tray-only at 1 s

On this run the coalition was 0.33 to 0.60% tray-only in parallel runs, 0.68 to 1.0% in alternating runs. That is not reliably under the target. The tray main thread is now about 0.1 to 0.26 points and the engine 0.2 to 0.28 points. The options D-067 lists, none of them taken:

1. Redraw at most every 2 s while sampling at 1 s: about half the main-thread tray cost, 0.05 to 0.13 points. The menu bar updates at half the rate.
2. A 2 s default interval: roughly halves both the tray and the engine, about 0.2 to 0.25 points.
3. Processes every 30 s with no window: most of `processes`' 0.09% (D-067), about 0.06 points.
4. hid.thermal and smc.sensors every 10 s when the menu bar shows no temperature: about 0.04 points, but the default menu bar shows the temperature, so this does nothing for the default.

A cheaper redraw path than `setImage:` (drawing into a layer of our own) would skip AppKit's replicant updates, which other displays' menu bars need, and its template tinting. Not pursued.

### Consequences

- tray-icon's own record of the icon goes stale after a swap. Nothing reads it: Kelvo never hides and re-shows the status item.
- The pinned length keeps the padding AppKit chose at the last size change. If macOS changed status item spacing while Kelvo runs, the item would keep the old width until its next size change.
- `perf-budget.json` `coalition.baseline` drops from 1.41 to 1.01, the highest clean alternating `make bench` run of the change.

### Revisit when

- A user decision on options 1 or 2 above.
- tray-icon gains a raw-image setter, or stops the PNG round trip: drop `set_status_item_image`.

## D-074: The data directory is owner-only

Status: Accepted. Date: 2026-10-05. Security review.

### Context

The app data directory (`~/Library/Application Support/com.tryopendata.kelvo`) holds usage history: which processes ran and when, CPU, power, network and disk activity over months, plus the host id, `settings.json` and `power-calibration.json`. Everything was created with the process umask, so the directory was 0755 and every file 0644 (`history.sqlite`, its `-wal` and `-shm`, `history.sqlite.lock`, `host-id`, `settings.json`). `~/Library` itself is 0700 on macOS, which keeps other local users out today, but that is a property of where the directory sits, not of Kelvo; a v4 agent's data directory on Linux (`~/.local/share`, often 0755) has no such parent.

### Decision

- The data directory is created 0700 and tightened to 0700 on every launch if it is looser (`kelvo_store::create_private_dir`).
- Files are created 0600; an existing file with group or other bits is tightened to owner-only on open. Modes are only ever tightened, never loosened.
  - `kelvo-store`: `Store::open` creates the database file 0600 before SQLite opens it (SQLite takes an empty file as a new database and creates the `-wal` and `-shm` files with the database's mode), tightens an existing database, `-wal` and `-shm`, and opens the lock file 0600. The store does it itself so a v4 agent gets it without the shell.
  - `host-id` and `power-calibration.json` are written through a 0600 temp file and a rename (`kelvo_store::write_private`) and tightened when read.
  - `settings.json` is written by `tauri-plugin-store`, which creates it with the default mode and rewrites it in place (keeping the mode). The shell tightens it at launch and after each save.
- The helpers live in `kelvo-store` (`perms.rs`), the crate that owns on-disk data, and use `std::os::unix` only: no new dependency, no `unsafe`.

### Consequences

- A data directory copied from another account or restored with loose modes is fixed on the next launch.
- A file the user made read-only for the owner (0400) stays that way; tightening never adds bits.
- Not covered: `.window-state.json` (written by `tauri-plugin-window-state`; window positions, not usage) and the log directory. Both sit in a 0700 directory or under `~/Library`.
- Moved-aside databases (`history-reset-<ms>.sqlite`) keep the mode they had, which is 0600 once the store has opened them.

## D-075: `make perf` runs one engine at a time with a 30% band; process-row allocations no longer follow process churn; dependency audit in CI

Status: Accepted. Date: 2026-10-05. Phase 7 (performance). Amends D-062 and D-067. Review fix (#15) and two gate fixes.

### Context

- **#15, `make perf` noise.** The engine baseline is 0.38% with a 10% band (ceiling 0.418%). Identical engine code has measured 0.15% (D-070), 0.25 to 0.38% (D-067) and, in this sitting, 0.159 to 0.222%. The 10% band sat inside that spread, so a busy sitting could fail the gate with nothing changed. `scripts/perf.sh` also ran the 30 s comparison engine beside the gated 1 s one, so each engine's processes collector read the other and they competed for cores.
- **Flaky allocation gate.** `allocsPerTick.window.collectors.processes` read 1.9 to 2.1 against a limit of 1 on some runs and 0.96 on others.
- **WAL budget.** D-070 measured 4.04 MB of WAL appends per hour and left the gate to assert it once the budget named it.
- **No dependency audit.** Nothing checked advisories, licenses or sources for the Rust or npm trees.

### Decision

**The engine CPU gate (`scripts/perf.sh`).**

- The runs are sequential: the gated tray-only 1 s run (and its retry when over), then the 30 s run, reported only.
- A run that ticked under 90% of the expected count is not a result. The script exits 2 and prints the power source and load average. The engine backs off to 2 s on battery and in Low Power Mode, and a saturated machine misses ticks. Earlier in this sitting five runs ticked 51 of about 105 times each and measured 0.12 to 0.23% for half the work. Their cause was not captured: the power source was not logged, and the machine was on AC with load average over 100 when checked afterwards. The next runs ticked 104 times. The old check only caught runs under 50%.
- `engine.perf.regressionPct` goes from 10 to 30, so the ceiling is 0.494%. The baseline stays 0.38%, the highest measurement of current code. The band covers the widest spread measured within one sitting (1.52x in D-067, 1.40x here) on top of that high. The gate still fails only when two runs in a row are over.
- **What the gate can catch.** It catches a regression of about 0.1 points over the highest reading, or about 2.5 times today's typical 0.19%. It cannot catch a 10% change. A finer claim still needs parallel runs of the old and new build in one sitting (`.claude/rules/verification.md`).

Alternatives considered:

- **CPU per process enumerated.** Only the processes collector scales with the pid count, and it is about 20% of the engine (0.03 to 0.04 of 0.17 to 0.22 points). The rest is SMC, HID and IOReport, which don't scale with it. Dividing by pids would hide a regression anywhere else.
- **A reference run of the previous build in the same sitting.** This is the right tool for measuring a change. As a gate it needs a second checkout and build and doubles a 4-minute run. Its noise between two runs is still the 1.4x measured here.

**Measured** (dev M3 Max, macOS 27, about 1,150 processes, other agents building, load average 5 to 117; release `dump --perf`, tray-only at 1 s, 120 s each, on AC):

| Runs | Engine CPU |
|---|---|
| Sequential, five | 0.185, 0.222, 0.222, 0.168, 0.175% |
| Parallel with a 30 s engine (the old layout), two | 0.159, 0.183% |

The parallel 30 s engine did not raise the 1 s reading measurably here (both pairs sit inside the sequential spread). Running the engines one at a time removes a confounder; it is not where the noise came from. Load is.

**The flaky processes allocation count.** The cause was not a cache recheck. Per-sample counts in the gate showed allocations only in samples where processes had started, in multiples of three, plus one in samples with more rows than the last. The fake ticker runs 120 ticks in about 2 s of wall time, so a machine running builds (dozens of short-lived compilers a second) starts many processes inside the measured window, and a quiet machine starts few. Two code fixes:

- `libproc::name_of` returns a `Cow<str>` borrowed from the BSD info. A new process's name was a `Vec<u8>`, then a `String`, then the `Arc<str>`; it is now the `Arc<str>` only (3 allocations to 1).
- `SampleBuf::take_processes` leaves room for an eighth more rows than the batch it hands off. At exactly the last size, any sample with one more row than the last reallocated the vector.

Window-open gate, 5 or 6 runs each under the same build load: before 0.18 to 1.63 allocations per tick (two of six over 1), after 0.17 to 0.42. The limit stays 1. What remains is one name per process that starts, which the gate now prints as "new process rows". It would take about 120 process starts in the 2 s window to reach the limit. The wrapper's own bookkeeping for that count is excluded from the engine core figure.

**Budgets.**

- `engine.store.walBytesPerHour` is 5,000,000 (measured 4.04 MB/h at the 5-minute commit interval, D-070). `store_write_volume_per_hour_at_one_second` asserts it; it is no longer optional.
- The live channel's "dashboard shown after half an hour hidden" Playwright case reads `frontend.hiddenResume.longTaskMs` (50, the value it used before) instead of `dashboardOpen.longTaskMs`.

**Dependency audit.**

- `deny.toml`, checked by `cargo-deny`. Graph: aarch64 and x86_64 macOS plus x86_64 Linux, all features.
  - Advisories: vulnerabilities and yanked crates fail. Unmaintained and unsound advisories fail only for direct dependencies; a transitive one (glib in Tauri's Linux GTK stack is unsound today) is reported.
  - Licenses: an allow list of permissive licenses plus MPL-2.0, which covers the whole tree today. Kelvo has no license yet, and our private crates are not checked. Everything on the list is compatible with MIT, Apache-2.0 or GPL for the app.
  - Bans: duplicates warn, because Tauri's tree carries several majors of base64, syn, toml, hashbrown and windows-sys. A second libsqlite3-sys, rusqlite, serde, tauri or tokio fails. Wildcard versions fail, except our path dependencies.
  - Sources: crates.io only.
- CI job `deny` runs `EmbarkStudios/cargo-deny-action` (v2.1.1, cargo-deny 0.20.2) as two checks. `bans licenses sources` blocks. `advisories` is shown but does not fail the run, because a newly published advisory would otherwise fail unrelated PRs; this is cargo-deny's documented pattern.
- CI job `audit-frontend` runs `bun audit` (Bun 1.4.2), also non-blocking for the same reason. Locally it reports no vulnerabilities in 270 packages.
- `make deny` runs `cargo deny --all-features check` when cargo-deny is installed and otherwise skips with a message.
- cargo-deny was not run locally (not installed). The config was checked against the 0.20.2 documentation. A rough pass over the RustSec database against `cargo metadata` found no vulnerability that the locked versions are not patched for.

### Consequences

- `make perf` takes about 4.5 minutes instead of 2.3, and up to 6.5 when the first run is over the ceiling.
- `make perf` on battery or in Low Power Mode exits 2 with no verdict, where before it passed on half the work.
- The engine CPU gate catches only large regressions. The coalition gate (`make bench`, 10%) and parallel same-sitting runs remain the tools for small ones.
- A new advisory shows as a failed, non-blocking check until someone updates the crate or adds a reasoned `ignore` entry.

### Revisit when

- A self-hosted Apple Silicon runner exists. With a quiet machine and repeated runs, the band can narrow.
- Kelvo picks a license: check the `deny.toml` allow list against it.
- The processes collector stops allocating a name per new process, for example by interning names. Then the window limit can drop toward 0.

## D-076: History older than 7 days is kept in 15-minute buckets

Status: Accepted. Date: 2026-10-05. Store. Amends D-057 (budget and cap) and D-064 (sync row kinds). Amended by d496112: the Timeline and size projection follow-ups in Consequences are done.

### Context

The 30-day fill measured 139.7 MB for 150 series and 203.4 MB for 250 (D-057), so a wide host relied on the byte cap and lost about two weeks of history to it. Almost all of that was 30 days of `tier_1m`. Nothing on screen needs minutes beyond a week: the 7d range draws about 2,000 points and the 30d range about 2,000, which is 15-minute resolution or coarser.

### Decision

**Tiers.** `Tier::M15` (`"m15"`, 900,000 ms buckets, persisted) and schema version 3:

- `tier_15m`, the same shape as `tier_1m` (`host_id`, `bucket_ts`, `layout_id`, `seq`, `blob`), with `tier_15m_key` unique on `(host_id, bucket_ts, layout_id)` and `tier_15m_seq` on `(host_id, seq)`.
- `proc_top_15m`, the same shape and key as `proc_top_1m`.

The migration only creates the tables. The next prune rolls down existing minutes.

**Retention.** `Retention` gains `history_ms` (the user's setting: 7, 30 or 90 days). `m1_ms` becomes the M1 window, fixed at 7 days and capped by `history_ms`. `tier_15m`, `proc_top_15m`, gaps and events are kept for `history_ms`. With 7-day retention nothing is rolled down.

**Roll-down** happens in the prune, for every host, in one-day chunks. Each chunk is one transaction that writes the 15-minute rows and deletes the minutes. Minutes older than the 15-minute boundary at or before `now - 7 days` fold into one row per `(15-minute bucket, layout)`:

- min of mins and max of maxes;
- the mean of the averages of the minutes that had a value. The blob has no sample counts, so each sampled minute weighs alike;
- NaN where no minute in the 15 had a value, never zero.

A layout change inside a quarter gives two rows for that quarter, as it does for minutes. Rows that do not decode to their layout's width are logged and dropped. Gaps stay as rows and nothing is filled in across them. The insert is `ON CONFLICT DO NOTHING`, so a 15-minute row that sync already delivered is kept as the agent made it.

The `pruned` row `m1_rolled` records the roll cut and the highest rolled `seq`.

**Process history.** `proc_top_1m` is kept for 7 days. After that it rolls down to `proc_top_15m`: the top 5 by mean CPU over the minutes present, the same rule the minute roll-down from snapshots uses. `processes_at` falls back to it with `ProcessResolution::Top5Per15Minutes` (`"top5_per_15_minutes"`). Keeping `proc_top_1m` for 30 days instead would have cost about 5 MB and the tooltip's process list would have stayed per-minute. We rolled it down anyway so that one rule covers everything older than a week.

**Reads.**

- `TierChoice::Auto` picks S10 while it covers the start of the range, then M1 while the start is at or after the roll cut, else M15. `TierRequest::Auto` follows it.
- An M15 read unions `tier_15m` with the minutes still in `tier_1m`. Both fold into the same 15-minute slots, weighed by width (a quarter row counts 15, a minute counts 1). A 30-day range therefore has no seam or hole at the 7-day line.
- `TierRequest` gains no fixed `m15` option. `auto` covers every use found.

**Sync (D-064).** A new row kind, `SyncRowKind::M15`, with the feature `rows.m15`. M15 rows travel on their own `(host, M15)` cursor only when both sides negotiated it. For an M1 cursor:

- With `rows.m15` negotiated, the roll-down is not a truncation. What the cursor missed arrives on the M15 cursor.
- Without it, a cursor whose `seq` is below the `m1_rolled` seq gets `Truncated` at the roll cut, and the receiver writes a `truncated` gap. It never sees a silent hole.
- A fresh cursor reads whatever minutes exist, as after an ordinary prune.

Older builds decode `"m15"` as `Tier::Unknown` (D-040). That is pinned by a pre-M15 decoder test against the new `sync_request_m15` fixture.

**Cap and low-disk guard (D-057, D-059).** The trim takes its oldest and newest points across `tier_1m` and `tier_15m`. It cuts on 15-minute boundaries and deletes the 15-minute history first (it is older), then minutes, and moves the M1, M15 and S10 `pruned` marks. The low-disk guard is unchanged.

### Measurements

`make test-fill` (release, 31 simulated days, daily prune, 16 KiB pages):

| Series | Before (D-057) | After |
|---|---|---|
| 150 | 139.7 MB, no trim | 69.9 MB, no trim |
| 250 | 139.9 MB after the cap trimmed it (203.4 MB uncapped) | 95.7 MB, no trim |
| 250 with a 92 MB cap | | 91.2 MB, trim met; the oldest 13 days of quarters and 1 day of minutes trimmed once |

After the change, `tier_1m` holds 10,080 rows, `tier_15m` 2,208, `proc_top_1m` 5,760 and `proc_top_15m` 2,208. What remains is mostly the 7 days of minutes, 24 h of 10 s buckets and 72 h of snapshots.

### Consequences

- A day older than 7 days has 96 points per series. The Timeline's 24h range should request `auto` (it requests `m1` today and would be empty there). The v1.1 heatmap's cell click opens about 6 h around the hour for such days, not 1 h. The 30d range reads `tier_15m`.
- Settings' projected size per retention (`history-projection.ts`) is fitted to the old fill and overstates 30 and 90 days.
- Amended by d496112: the Timeline's 24h range now requests `auto` and draws 15-minute buckets for a day older than 7 days, and `history-projection.ts` is refitted to the D-057 and D-076 fill pairs (90 days at 150 series is about 83 MB). The heatmap note above still applies to v1.1.
- A controller that has not negotiated `rows.m15` loses minutes older than 7 days that it had not synced before the agent rolled them down. It gets a gap for them, not data.
- Each prune does one more pass, and on first launch after the upgrade it rolls down up to 23 days of minutes in one-day transactions. At 150 series one day is 1,440 rows read and 96 written.

### Revisit when

- A screen needs minute detail older than a week: make the window a setting, not a constant.
- Blobs gain sample counts: weight the roll-down by them.

## D-077: The tray redraws at most every 2 s while sampling stays at 1 s

Status: Accepted. Date: 2026-10-05. Phase 7 (performance). Takes option 1 of D-073's "what is left".

### Context

After D-073 a drawn tray frame still costs the main thread 5 to 8 ms, almost all of it in AppKit after `setImage:`. At a 1 s tick the frame skip leaves 30 to 48 drawn frames a minute. The default interval stays 1 s (D-073 option 2 was not taken), so sampling, history and the live views are unchanged.

### Decision

- `tray::model::Pacer` replaces `FrameSkip`.
  - A frame equal to the last drawn one is skipped, as before, and also cancels any held frame.
  - A changed frame is drawn when one of these holds:
    - the redraw period has passed since the last draw (less 250 ms, so a jittered 1 s tick draws every other tick instead of waiting on the timer);
    - nothing has been drawn yet;
    - it is urgent.
  - Otherwise it is held. Only the newest held frame is kept.
- The period is 2 s. It is 4 s while the engine is backed off for battery or Low Power Mode, which doubles on top of the engine's own 2 s back-off tick.
- The no-stale-image guarantee: the tray thread now runs a current-thread tokio runtime and selects between the bus and a timer at the held frame's deadline, which is the last draw plus the period plus 0.5 s. If no newer frame arrives first (the frames stopped, or the user's interval is longer), the timer draws the held frame. While frames flow at 1 s, the next frame always comes before the deadline, so the timer never fires.
- Pausing is urgent and draws at once, as before. Gaps need nothing extra: a gap's frame (dashes, empty tracks) is a changed frame like any other, so it shows within one period. Display idle drops the held frame and draws nothing until wake. The first changed frame after wake draws on arrival, because the period has long passed.
- A settings change still shows with the next frame, as before. It is not treated as urgent.

### Measurements

**Not benchmarked with `make bench`.** The user considers performance settled, and no `make bench` or gate run was made for this change. `perf-budget.json` is unchanged.

Before that call, five parallel pairs had already run in one sitting, following D-073's method:

- HEAD (D-076) as `com.tryopendata.kelvo.bench` and the change as `com.tryopendata.kelvo.bench2`.
- Both bundles had the same prebuilt frontend and the same settings (1 s, default menu bar).
- Tray-only, 120 s each after 30 s warm-up, measured at the same time, with a discarded first pair to absorb the v3 migration and first prune.
- Dev M3 Max, macOS 27, on AC, with other agents building. Load average 24 to 152.

| Pair | Coalition before | Coalition after | Main thread before | Main thread after |
|---|---|---|---|---|
| 1 | 0.799% | 0.654% | 0.421% | 0.281% |
| 2 | 1.260% (kelvo process 0.855%) | 0.595% | 0.425% | 0.240% |
| 3 | 0.518% | 0.462% | 0.247% | 0.191% |
| 4 | 0.690% | 0.525% | 0.382% | 0.231% |
| 5 | 1.005% | 0.685% | 0.610% | 0.322% |

- In pair 2 the "before" app's WebKit WebContent and GPU helpers used 0.40 points, which a tray-only run should not. The kelvo process alone is the fair comparison there.
- The main thread dropped 0.06 to 0.29 points (23 to 47%) in every pair. The engine thread was the same in each pair (0.23 to 0.33%).
- The coalition, compared on the kelvo process where the helpers misbehaved, dropped 0.06 to 0.32 points.
- With the change the coalition read 0.46 to 0.69% tray-only. That is still not reliably under the 0.5% target at this load.
- Absolute numbers moved by 2x with load across the sitting; only the within-pair differences mean anything.

### Consequences

- The menu bar changes at most every 2 s (4 s on battery). A spike shorter than that can be missed in the icon, but not in history or the popover, which still update at the base tick.
- One more thread-local runtime, on the tray thread. It holds no timer unless a frame is held.
- Any change to `coalition.baseline` waits for a `make bench` run, which was not made.

### Revisit when

- A user wants a 1 s menu bar: make the period a setting rather than reverting.
- A cheaper redraw path than `setImage:` appears (D-073): the period could drop back to the tick.

## D-078: Release infrastructure moves after v1.2 and manual QA; performance is accepted as it stands

Status: Accepted. Date: 2026-10-05. User decision. Restructures v1-local-monitor.md section 8.

### Context

v1.0 phases 0 to 5 are done and the app runs well from source. Phase 6 mixed perf gates (done) with distribution: the DMG, ad-hoc signing, the minisign updater, the release workflow, the Homebrew tap, the Gatekeeper check, the Stats comparison and the packaged-app checklist. The user wants to use the dev build day to day and iterate on bugs and UX before any release path exists.

### Decision

- Order: v1.0 (done) → v1.1 → v1.2 → v1.x QA and polish (manual, from source, several bug-fix and UX sessions) → v1.x release (distribution, updater, install).
- No tags, releases or updater builds before the release phase. Phases end with a build that runs from source.
- Phase 6 keeps only its done items (perf gates, dependency check) and is renamed "benchmarks and gates". Its open items move unchanged to the release phase.
- Manual checks that don't need a packaged app (lid-close gap, Reduce Transparency, fullscreen first-show, display wake, ⌘-drag, fan mode, disk rates, calibrated CPU power, CSP in a local build) move to the QA phase.
- Performance is accepted as it stands. After D-073 and D-077 the whole app measured 0.46 to 0.69% tray-only under heavy load in parallel runs, against the 0.5% target; the user considers that fine. The gates in `perf-budget.json` stay as regression guards. No session works on overhead again unless a gate fails or a feature adds cost.

### Consequences

- The release phase still owns `make bench-vs-stats`, the budget table run and the accuracy run on macOS 26 and 27.
- The 0.5% target in architecture.md is unchanged as a target; it is not met reliably on a loaded machine, and that miss is accepted here.

## D-079: Long-range reads report their slot width; the heatmap takes local hours from the frontend; CSV export streams from the store

Status: Accepted. Date: 2026-10-05. Store and shell. Builds on D-076 (reads at 15-minute resolution) and D-070 (commit interval).

### Context

v1.1 adds the 7d and 30d Timeline ranges, a 30-day heatmap by local hour, and a CSV export (v1-local-monitor.md, phases 1.1-A to 1.1-C). Three things were open: how the Timeline learns the width of a merged point, where local time and DST are computed for the heatmap, and how an export of 30 days avoids holding the range in memory.

### Decision

**Slot width.** `HistoryResult` and `HistoryPage` carry `bucket_ms`: the tier's bucket times the buckets merged into one slot to stay under `max_points`. A 7d range at 2,016 points reads `tier_1m` at 5 minutes, at 1,008 points at 10 minutes ("10 MIN AVG"); a 30d range at 1,440 to 2,200 points reads `tier_15m` (minutes folded in, D-076) at 30 minutes. Slots still start at the bucket holding `from_ms`, not at epoch multiples of the slot width; see Revisit.

**Heatmap.** The store stays time-zone agnostic. `Reader::heatmap(host, series, cells)` averages one series over caller-given `[start, end)` cells from `tier_15m` and `tier_1m`, each bucket weighed by its width in minutes, `None` where nothing was sampled. `query_heatmap` takes `HeatmapRequest { host, metric: cpu | temp, days: { date, hour_starts }[] }`, where `hour_starts` is 25 UTC instants per local day (hours 00 to 23, then the next midnight), built by `heatmapDays` in `src/core/heatmap-days.ts` from the JS `Date`. A spring-forward hour that does not exist is an empty cell (null); a fall-back hour that happens twice is one two-hour cell. Every zone in use is offset by a multiple of 15 minutes, so no bucket straddles a local hour. At most 92 days per request; boundaries must never decrease (`invalid_argument` otherwise). The alternative, an IANA zone name sent to Rust, would have added a tz database dependency to compute what the webview already knows.

**CSV export.** `Reader::export_csv` writes the CSV itself while it reads, inside one read transaction so every pass sees the same snapshot (a commit between passes could otherwise add a layout the header never named, or move minutes into `tier_15m` mid-export). A first pass over the `(host_id, bucket_ts, layout_id)` key finds the layouts in range so the header can name every column, then one statement per table (`tier_15m` and `tier_1m` for an M15 export) streams rows in bucket order, merged, one bucket's values in memory at a time. One row per bucket of the tier `auto` picks that has a value for at least one selected series; no merging. Columns: `time_utc,time_ms`, then `<series>_avg,_min,_max` per series, then `gap_reason,gap_end_ms`. Gaps are rows of their own at their start (or the range start), with empty value cells, `gap_reason` (`module_disabled:<module>` for a module) and `gap_end_ms` (empty while the gap is open). Header names with commas are quoted. `export_csv` (shell) opens the save dialog from Rust with `tauri-plugin-dialog` on a blocking thread, as a sheet on the calling window, with a file name the frontend proposes (local dates are the frontend's to format). No window is granted dialog permissions. A dismissed dialog is `{ kind: "cancelled" }`, not an error; a file that cannot be created or written is `CommandError::Export`.

### Measurements

`make test-read-perf` (release, the 150-series 30-day fill from `make test-fill`, dev M3 Max, each read on a new connection; thresholds in `perf-budget.json` `store`):

| Read | Measured | Budget |
|---|---|---|
| Timeline 7d, six lane queries, 2,200 points each | 34.5 ms | |
| Timeline 30d, six lane queries | 38.6 ms | 500 ms with the heatmap |
| Heatmap, 720 hourly cells | 4.1 ms | |
| CSV export, 30 days, all 150 series, 2,880 rows, 9.7 MB | 75.1 ms | 3 s |

### Consequences

- The CSV format lives in `kelvo-store` next to the streaming read; the shell only owns the file and the dialog.
- `make test-fill` runs the read budget as well, so CI checks it on Linux.
- The Timeline must use `bucket_ms` for its resolution label rather than assume the tier width.
- Export flushes the writer before it reads, so the up to 5 minutes the writer holds uncommitted (D-070) are in the file rather than silently missing at the end of a Live export.
- Export writes to a hidden sibling `.kelvo-<pid>-<n>.tmp` (short, so a target name at the 255-byte limit still works, and unique per export, so two exports to one path never share it), `sync_all`s it and renames it over the chosen path. A failed export removes only the temporary file, so a file the user agreed to replace survives the failure; a crash leaves one hidden temporary file per interrupted export (nothing cleans old ones up), never a truncated CSV under the chosen name.
- A sandboxed App Store build would need `com.apple.security.files.user-selected.read-write`, and that grant covers only the chosen URL, not its directory, so the sibling temp file is not allowed. That build would write the temporary file in its own container and copy it into the chosen URL at the end (or write the chosen file directly), giving up the atomic replace.
- The heatmap's current day keeps filling in: the Timeline (the Timeline UI lane is implementing this) refetches it every 5 minutes (`HISTORY_COMMIT_MS`, matching the commit interval) and when the local hour changes.

### Revisit when

- The Live 7d or 30d view refetches as the window slides: slot boundaries move with `from_ms`, so merged values shift slightly between fetches. Aligning slots to epoch multiples of a "nice" width (5, 10, 15, 30 minutes) would hold them still.
- A screen needs the heatmap in a zone other than the webview's.

## D-080: Own-item menu bar modes: one status item per module, graphs from board 01, history in the tray

Status: Accepted. Date: 2026-10-05. v1.1 phase 1.1-D. Builds on D-037 (autosaveName) and keeps D-077 (2 s redraw).

### Context

v1.1 adds the "Graphs" and "Cores + histogram" rows of board 01 and per-module menu bar items with ⌘-drag reorder. v1.0 drew every element into one status item, chosen per module by `MenuBarMode` (In combined item, Value + label, Temp in combined, Watts as value, Hidden). The plan names two new modes, "Own item: graph" and "Own item: value", and leaves open which modules get which.

### Decision

- `MenuBarMode` gains `OwnGraph`, `OwnValue` and `OwnCores` ("Own item: graph", "Own item: value", "Own item: cores"). `allowed_for`:

  | Module | Own modes | Graph |
  |---|---|---|
  | CPU | graph, cores, value | sparkline of the last 20 samples; cores is the per-core strip, P cores then E cores with a 3 pt gap |
  | GPU | graph, value | history bars of the last 9 samples |
  | Memory | graph, value | fill gauge plus percent |
  | Network | graph, value | sent over received rates, stacked, right-aligned |
  | Power & Sensors | value | none: watts with the PWR label |
  | Disk, Battery | value | none |

  Board 01 draws no graph for Power, Disk or Battery, so they get the labelled value only. Power's own item shows watts (as `WattsValue` does), not the temperature. `OwnCores` is CPU-only; on a host with no `cpu.load` series it falls back to the value.
- Each module in an own mode gets its own status item. Its tray id doubles as its `autosaveName`: `kelvo` (combined), `kelvo-cpu`, `kelvo-gpu`, `kelvo-memory`, `kelvo-power`, `kelvo-network`, `kelvo-disk`, `kelvo-battery`. The name is set right after creation through `ns_status_item()` (D-037). In-combined and value-label modes still draw into the combined item.
- The combined item exists while it has something to show. When every shown module has its own item, it is removed. With nothing shown at all, it keeps the three empty tracks so there is always something to click.
- The tray thread owns the set of items. Every build compares the wanted keys to the shown ones, creates the missing items (last to first, since macOS adds a new item to the left of Kelvo's others) and removes the extra ones on the main thread. A `settings-changed` listener rebuilds at once, so adding or removing an item does not wait for the next frame and works while paused.
- The items share one `Pacer`, with D-077's 2 s period (4 s backed off). Frame skip is per item: an item whose frame equals what it shows is not drawn. All the items that changed draw together, at most once per period, and one held-frame timer draws every held item at the shared deadline. A just-created item draws on arrival. Each item's draw is its own main-thread call (the D-073 swap, or tray-icon's path on a size change), as before the shared pacer. One call for all due items was tried and dropped: with every module in its own item it holds the main thread for up to seven `setImage:` calls at once (5 to 8 ms each, so up to about 55 ms every 2 s), and the measurements below show it saved nothing measurable. `ItemState` stays per item.
- The create/remove decision is `model::item_change`, a pure function with unit tests. An item whose `TrayIconBuilder::build` fails is logged once per failure streak and tried again every 30 s (`model::Retries`), and at once when the settings change or the display wakes. The tray thread's timer wakes for the retry, so it also happens while paused.
- The sparkline and history bars read `TrayHistory`, bounded rings (20 and 9 samples) that every bus frame feeds. Nothing queries history. A gap is a `None` slot: the line breaks and the bar is empty. Pausing and display sleep clear the rings, because the next sample is not next to the last one in time.
- The menu handler is registered once on the app (`app.on_menu_event`), not per item. Tauri keeps per-tray menu handlers as global listeners, so per-item handlers would toggle Pause once per item. Every item shares the one menu. A left click opens the popover under the item that was clicked: `popover::toggle` takes the tray id.
- The network rates use 8 pt text on a 9 pt pitch (the design-system figure). Board 01's 9 px text on a 10 px line does not fit the 18 pt image once "/" and the arrows reach past the caps.
- Onboarding's "Graph per module" card presets board 01's Graphs row: CPU, Memory and Network `OwnGraph`; GPU and Power & Sensors hidden, along with Disk and Battery.

### Consequences

- More items mean more `setImage:` calls. Each costs AppKit about 5 to 8 ms of main-thread time per drawn item (D-073). Sharing the pacer lines the draws up in time; it does not remove that work: a graph item changes on almost every tick (the sparkline shifts, the rates move), so with the Graph per module preset three items draw every 2 s. The measurements below show the cost.
- Whether ⌘-drag order persists across launches with these names is still the manual check D-037 asks for.
- A newly added item appears at the left of Kelvo's items until the user moves it. macOS keeps that position under its autosave name from then on.

### Measurements

`make bench`'s tray scenario (`scripts/bench-coalition.sh`, `com.tryopendata.kelvo.bench`, `NO_BUILD=1 SCENARIOS=tray`), 90 s after a 20 s warm-up. Two bundles: "per item" is the first D-080 commit, with a pacer and main-thread hop per item; "shared" is the second, with one shared pacer and one main-thread call for all due items. The shipped form, a shared pacer with a hop per item, was not benchmarked separately: it differs from "shared" only in the batching that measured as noise. Each was run with the bench identity's settings set to the default combined item, the Graph per module preset (CPU, Memory and Network `OwnGraph`), and every module in its own item (CPU, GPU, Memory and Network `OwnGraph`; Power, Disk and Battery `OwnValue`). Two rounds, alternating order. Dev M3 Max, macOS 27, on AC, other agents building, load average 2.6 to 47. Coalition CPU and the kelvo main thread, % of one core:

| Menu bar | Per item, coalition | Shared, coalition | Per item, main thread | Shared, main thread |
|---|---|---|---|---|
| Combined | 0.917, 0.812 | 0.880, 0.862 | 0.406, 0.423 | 0.433, 0.449 |
| Graph per module | 1.987, 1.006 | 1.751, 1.260 | 1.159, 0.591 | 0.953, 0.769 |
| Every module own | 2.430, 1.896 | 1.942, 1.761 | 1.623, 1.365 | 1.380, 1.229 |

- Own items cost real CPU. Against the combined item, the Graph per module preset adds about 0.2 to 1.1 points and every-module-own adds about 0.9 to 1.5. Nearly all of it is the main thread, which is AppKit redrawing each item.
- Sharing the pacer and batching the draws did not measurably change the Graph per module preset: it read lower in one round and higher in the other. With every module in its own item it read 0.14 to 0.49 points lower in both rounds. Load swung between runs, so that is a hint, not a result.
- WindowServer's own CPU over the same windows, from `ps` cputime, read 20 to 37% of a core whatever the configuration. That is the whole desktop under this load, so the 0.2% WindowServer budget cannot be checked here: the WindowServer re-measure is still open.

### Revisit when

- Own items are to meet the 0.5% tray-only target: a longer redraw period for own graph items, or fewer redrawn pixels per tick (a sparkline that moves every other draw), are the levers. Sharing the pacer and batching the main-thread calls were not.
- A quiet machine is available: measure WindowServer with every module in its own item against the 0.2% budget.
- The manual ⌘-drag check fails: store a preferred order and recreate items in it (D-037's fallback).

## D-081: Per-process network uses NetworkStatistics in process; it sees only the current user's flows

Status: Accepted, amended by D-089. Date: 2026-10-05. Collect and engine. Settles the open question in v1-local-monitor.md phase 1.2-A ("unverified that it works unprivileged on current macOS"). Amended after the v1.2 code review (bullets marked "amended after the v1.2 review").

### Context

v1.2 shows per-process network rates (Overview Network card top 5, Network and Processes page columns). The plan named the private NetworkStatistics framework (`NStatManager`) and said that if it is unavailable unprivileged, the columns are hidden. A spike checked it before any collector work.

### Spike

A throwaway Rust binary (not in the repo) on macOS 27 (Darwin 27.0.0, Apple Silicon), run as the logged-in user, unsandboxed, ad-hoc linker-signed and again after `codesign -f -s -`, with identical results:

- The framework is only in the dyld shared cache; `dlopen` of `/System/Library/PrivateFrameworks/NetworkStatistics.framework/NetworkStatistics` plus `dlsym` resolved every symbol: `NStatManagerCreate(alloc, dispatch_queue, ^(source, ctx))`, `NStatManagerAddAllTCP` / `AddAllUDP` (return 1 on success), `NStatManagerQueryAllSources` and `…Descriptions` (completion blocks), `NStatSourceSetCountsBlock`, `NStatSourceSetDescriptionBlock`, `NStatSourceSetRemovedBlock`, `NStatManagerDestroy`.
- Dictionary keys are exported CFString constants (read by `dlsym`, not hard-coded): `kNStatSrcKeyPID` "processID", `kNStatSrcKeyProcessName` "processName", `kNStatSrcKeyRxBytes` "rxBytes", `kNStatSrcKeyTxBytes` "txBytes", `kNStatSrcKeyProvider` "provider", plus `interface`, `ifLoopback`, per-link-type byte counters. Values are SInt64. The counts dictionary also carries pid, name and provider.
- During a curl download it reported curl at about 22.9 MB over 2 s; for processes both tools listed, cumulative totals matched `nettop` within live drift (rapportd 2421658/447125 in both).
- About 120 sources; `QueryAllSources` costs 0.6 to 1.1 ms wall and similar CPU per call (2.4 ms first call).
- No entitlement is needed. It sees only flows owned by the current uid. `nettop` also shows root and system-user daemons (kernel_task, mDNSResponder, launchd, network extensions) because it holds `com.apple.private.network.statistics`; an ad-hoc binary carrying that entitlement is killed at launch (exit 137), so the all-users view is not available to Kelvo.
- A closing flow gets one last counts callback just before its removed block, so its final bytes can be folded into a per-pid accumulator. Source pointers are reused after removal.

### Decision

Ship per-process network in v1.2 with the in-process API; the hidden-columns fallback stays for when the framework or a symbol is missing.

- `kelvo-collect/src/macos/nstat.rs` loads the framework with `dlopen`/`dlsym` into an `Option<Api>` once; any missing symbol or key is a missing capability. `unsafe` stays in that module. `Cadence::OnDemand`, `Entitlement::NetworkStatistics`, dropped in the `appstore` build.
- The manager exists only while a view with network process interest is visible: created (serial dispatch queue, all TCP and UDP sources) on the first such view, `NStatManagerDestroy`ed when the last hides, so idle cost is zero.
- Per tick: query, wait for the completion with a short timeout, per-source deltas plus the closed-flow accumulator, summed per pid, divided by the measured interval, loopback flows excluded. The first tick after creation sets the baseline and yields no rate.
- The UI labels coverage as the user's processes. System daemons' traffic is not attributed; where a list must add up, the remainder (interface total minus the per-process sum) is "System and other".

### Consequences

- The Overview Network top 5 and the per-process columns miss root daemons' traffic (mDNSResponder, software update, VPN tunnels run as root). On a typical single-user Mac the heavy hitters are user processes (browsers, Docker front ends, node), which is what the acceptance test compares.
- Key names are verified on Darwin 27 only; parsing is covered by fixture dictionaries, the live read is an `#[ignore]` test.

### Revisit when

- A macOS release removes or renames the framework or keys (the capability goes absent and the columns hide).
- Kelvo gains a privileged helper (v3+), which could provide the all-users view.

---

## D-082: Per-process network: describe flows to learn their owner, rows only for the user's processes, no remainder row

Status: Accepted, amended by D-089. Date: 2026-10-05. Phase 1.2-A. Amends D-081. Amended after the v1.2 code review (bullets marked "amended after the v1.2 review").

### Context

Building the collector D-081 describes turned up one thing the spike missed and left two choices open: whether lists need a "System and other" remainder, and what a zero rate means.

- **Owners of old flows.** D-081 said the counts dictionary carries the pid. It does only for flows opened after the manager was created, or after a description query. The spike always ran `NStatManagerQueryAllSourcesDescriptions` first, which hid this. Without it, 127 of 130 sources reported `processID` 0 in their counts, and the first live run of the collector put Claude Code's 150 KB/s upload on pid 0.
- **Which rows exist.** The processes collector emits only processes it can read, which are the current user's (D-045). NetworkStatistics sees exactly the current user's flows (D-081). So every row Rust sends is a process whose traffic is fully visible.

### Decision

- **Describe, then count.** The collector sets a description block on every source. The first sample after the manager opens runs a description query after the counts query; later samples repeat it, at most every 10 s, while some flow with bytes has no known owner. Nothing is attributed to pid 0. A flow whose owner is learned after the baseline counts from the bytes it had when the owner became known, so the bytes moved while its owner was unknown are dropped rather than charged to one sample as a spike of up to ten times the real rate (amended after the v1.2 review). A flow that is new since the baseline and names its pid in its first counts counts in full.
- **Timeouts and backoff** (amended after the v1.2 review). Each query waits for the first completion after it was issued, so a lost or merged completion cannot stall later queries. A query that times out (250 ms) drops the session, and the next sample makes a new manager and baseline; the failing sample is a `CollectError::Timeout`. After three failures in a row (timeouts or a failed start) the collector backs off for 60 s and rows show "Measuring" meanwhile; capabilities do not change at runtime. The manager is destroyed on its own serial queue (`dispatch_sync_f`), so callbacks already queued finish first; a live churn test (2,000 open and release cycles under loopback traffic) ran clean.
- **A zero is a zero.** On a measured sample, a row whose pid had no traffic gets `0`, not `null`. `null` means not measured: nobody asked (`ProcessView.network`), the baseline sample, or `Capabilities.process_network` false. The UI shows "Measuring" until the first measured batch.
- **No "System and other" row.** None of the three surfaces (Overview Network card top 5, Network page Processes section, Processes page Network columns) presents a list that must add up to the interface total, so there is no remainder row. Each says "Your processes only" instead, and the Network page adds that system daemons' traffic is not attributed. If a later view stacks per-process rates against the interface total, the remainder (interface total minus the per-process sum) appears there, labelled "System and other", as D-081 said.
- **Interest shape.** `ProcessView.network` asks for rates; the live registry tells the host whether any visible window with process interest wants them (`set_network_process_interest`). The engine samples `Cadence::OnDemand(Interest::NetworkProcesses)` collectors only on the ticks process rows are sampled, so the rates cover the rows' span, and calls `Collector::release` when either interest ends. `ProcessSort` gains `net_rx`, `net_tx`, `net_total`.
- **Budgets.** `perf-budget.json` gains a `windowNetwork` engine mode. The `trayOnly` and `window` modes allow zero NetworkStatistics calls per tick (the zero-idle-cost check); `windowNetwork` allows 1.2 (one counts query per process sample plus a description query at most every 10 s).

### Measurements

Dev M3 Max, macOS 27, about 1,130 processes, about 130 TCP and UDP sources.

- `perf_gates` (fake ticker, real collectors, 120 ticks): `trayOnly` and `window` 0 samples and 0 calls for `net_per_process`; `windowNetwork` 1.00 nstat calls per tick, 0 Rust allocations per tick in the collector, engine core 9.02 allocations per tick, the same as `window`.
- Release `dump --perf`, five engines side by side for 120 s on a machine at load average 38 (other builds), so whole-engine numbers are noise; the collector's own thread time is the reliable figure: `net_per_process` 0.0025% of a core with process rows every 5 s (21 samples), 0.0086% at every tick (104 samples), 0 samples tray-only and without network interest.
- One query: 0.6 to 1.3 ms wall; creating the manager plus the baseline (counts and descriptions): 2.9 to 3.2 ms.
- Live comparison with `nettop -P -d` over the same 6 s during a 6 MB/s-limited curl download: Kelvo 6.22 MB/s received for curl, nettop 6.33 MB/s (37,959,712 bytes in 6 s). Root daemons nettop lists (mDNSResponder, kernel_task) are absent from Kelvo, as D-081 expects.

### Consequences

- Up to 10 s after a long-lived flow's owner becomes known, one sample reports all its bytes since the baseline. In practice the first sample describes every flow, so this only affects a flow that reports pid 0 after its first description, which was not seen.
- The mock transport gives rates to every row it has, including other users' processes the real collector never sends. The rows Rust sends are only the user's.

### Revisit when

- A row for another user's process is ever emitted (a privileged helper, v3+): it would need `null`, not `0`, until NetworkStatistics can see its flows.
- A view compares per-process traffic with the interface total: add the "System and other" remainder there.

## D-083: Event detectors run on the persisted tick; events flush on write and reach windows as a pushed event

Status: Accepted. Date: 2026-10-05. Schema, store, engine, shell and Timeline. Settles the open choices in v1-local-monitor.md phase 1.2-C (flush-on-event or a live channel; attribution without forcing process sampling). Amended after the v1.2 code review (bullets marked "amended after the v1.2 review").

### Context

v1.2 adds `events` rows: fans ramped, thermal state changed, a process sustained high CPU, a package or ANE power spike, each attributed to process names, shown as Timeline pills and Power chart markers. The writer commits every 5 minutes (D-070), so an event written like a sample would reach `query_events` up to 5 minutes late, and a 1 Hz sampler must not pay for detection.

### Decision

- **Where:** a `Detector` trait in `kelvo-engine/src/detect/` (`bind` to the frame layout, `on_tick(values, process batch)`, `reset`). `Detectors` holds the four kinds (power spike once per component) and the alert rules (D-084), runs only on the persisted tick path, and resets on resume from sleep or pause, on a clock step and on a store change, so no episode spans a gap. Two events of one kind on one tick get distinct timestamps (+1 ms), since the store upserts on host, time and kind.
- **Thresholds are data:** `DetectorThresholds::DEFAULT` in `kelvo-schema` (fan rise 1,000 rpm in 60 s; thermal level held 10 s; a process at 100% or more for 2 minutes; power at 1.6x a 5-minute EWMA baseline and 8 W (package) or 1 W (ANE) above it for 10 s; refractory periods; attribution window and floor).
- **Fan ramps re-arm, they do not just time out.** A 25-minute real recording (`crates/kelvo-engine/tests/fixtures/busy-25min.json`) showed two failure modes of a plain 5-minute refractory period: a ramp inside the period was reported late when it ended, against a minimum from before the period, and back-to-back builds with the fans settling in between were missed. The detector now re-arms only once the fans fall back within half a rise of where the last ramp started and 2 minutes have passed, and measures the next ramp from that moment. Readings under 500 rpm are skipped: Apple Silicon fans stop at idle, and a fan starting at its minimum speed is not a ramp. On the recording it reports each of the five ramps once, while it happens. (Amended after the v1.2 review.) It also re-arms 10 minutes after a ramp (`fan_rearm_after_ms`), measuring the next ramp from where the fans are then, so fans that settle high after a ramp do not disarm it for good.
- **Bounded pills and sparse power** (amended after the v1.2 review). `sustained_process` reports a process name at most once per 10 minutes (`sustained_name_refractory_ms`), so a cargo build's many rustc processes give a few pills, not one per pid (a scripted 30-minute build: 28 before, 3 after). `power_spike` needs 10 readings (`power_warmup_readings`) before it is warm, and a hole of more than 3 sampling periods between `power.package` readings (`power_max_gap_periods`) restarts the warm-up and drops a hold in progress, since that metric is often a gap on macOS 27 (D-043). The period is the series' own, as the engine computes it for held values (D-047): IOReport reads every 10 s with only the tray open and every tick with a detail window, so 10 s spacing tray-only is normal, and across a cadence change the longer of the two periods applies, so opening a window does not restart the warm-up. At 10 s, 10 readings take 100 s, inside the 2-minute warm-up.
- **Attribution without forced sampling:** detectors read the process batches the processes collector already produces (every 10 s with no window open, every tick when a window wants processes) through a fixed-size `ProcessWindow` (top 5 by CPU and by energy per batch, last 256 batches). Attribution is a time-weighted mean over the window before the event: the top 2 by CPU (at least 20%) for a fan ramp, the top 1 by energy for a power spike. It allocates only when an event fires. No detector changes the processes cadence.
- **Flush on event, plus a push:** when a tick produces events the engine queues them, then `Writer::commit_soon()` (a non-blocking `Op::Commit` behind them), so `query_events` returns them within one writer round trip. The bus carries `BusMsg::Event`; the shell's host watcher emits `event-recorded` to every window. Windows merge the pushed event into each cached event list (`useEvents`, dedupe by kind and time) and do not refetch; a live list rereads every 5 minutes for anything a push missed. A separate live event channel was not needed: events are rare and the Tauri event already fans out.
- **Wire shape:** `Event { ts_ms, start_ms, processes, detail }` with `EventDetail` internally tagged by the `events.kind` text, stored as CBOR. There is no `Unknown` variant (D-040's usual fallback): `#[serde(other)]` cannot be exported by specta with serde phases off, and an event is a whole row, so the reader skips a row it cannot decode (a kind from a newer build) instead of failing the query.

### Consequences

- Per-tick cost is a few comparisons per detector; `perf_gates` measures 0 allocations per tick for the detectors with every alert on and a 1,000-row process batch (`engine.allocsPerTick.detectors: 0`), and the engine core's allocations are unchanged.
- On macOS 27, `power.package` and `power.ane` are gaps most of the time (D-043); the spike detector skips missing readings, so power spikes are rare on this Mac until those series come back. The real recording had three package readings in 25 minutes.
- Process names are stored, not pids, so a pill reads correctly after the process is gone; two processes with one name are not told apart.
- Optimized charging annotations (board 08) are not implemented: macOS documents no source for "charging held at 80%". `ioreg` `AppleSmartBattery` `ChargerData.NotChargingReason` is an undocumented bitfield that read 0x400001 at 100% charged here (not an optimized hold), and `pmset -g log` had no optimized-charging entries. The board's battery pill stays unbuilt rather than guessed.

### Revisit when

- Users report noisy or missed events: the thresholds are one struct, and the recorded fixture is the regression harness.
- `power.package` / `power.ane` come back on macOS 27 (D-043), to tune the spike thresholds on real data.
- A documented optimized-charging state appears in IOKit or a public API.

## D-084: Two built-in alert rules, off by default, posted through tauri-plugin-notification; a click cannot open the Timeline

Status: Accepted. Date: 2026-10-05. Engine, shell and Settings. Phase 1.2-D. Amended after the v1.2 code review (bullets marked "amended after the v1.2 review").

### Decision

- `AlertRule::hot_process` (a process above 200% CPU for 5 minutes) and `AlertRule::thermal_serious` (thermal state Serious or worse), both off, switched by `Settings.alerts` (two Settings rows under "Alerts", board 13 style). They are evaluated in the engine next to the detectors, edge-triggered, with a 30-minute cooldown per rule that survives detector resets, switching the rule off and on, and restarts (at start the shell seeds it from stored `alert` events of the last 30 minutes); a process run that completes inside the cooldown is spent, not reported late. When its progress is forgotten (a reset on sleep, a clock step or a relayout, switched off and on, a restart) while it is inside a reported episode, that episode still met when the rule next looks, within the cooldown, is the one already reported: it fires again only after the condition clears and holds again, never just because the cooldown ran out. For the process rule the episode is the processes the alert named (a restart reads them from the stored event); any other process is a new run. A reset outside a reported episode simply starts over. A fired rule is an `alert` event (D-083) and a notification.
- Notifications go through `tauri-plugin-notification` 2.5.1, posted from Rust only (no window holds its permissions). Turning a rule on calls `request_permission` and posts a one-off "Alerts are on" notification describing the rule, so macOS asks for permission while the user is looking rather than on the first real alert (amended after the v1.2 review; not yet seen in the running app). When several processes complete hot runs on the same batch, one alert names them all, hottest first.

### What the plugin does on macOS (read from its source, then observed)

- On desktop `request_permission` and `permission_state` return Granted without asking. macOS asks on the first delivery instead.
- It posts through `notify-rust` / `mac-notification-sys` (`NSUserNotificationCenter`) with `set_application`: Terminal's bundle id when `tauri::is_dev()`, the app identifier otherwise, ignoring a failure.
- Observed on macOS 27 with a scratch binary calling `notify-rust` 4.18.1 exactly as the plugin does (the dev app could not run from the worktree beside the user's running instance, which the single-instance plugin would hand off to): with `com.apple.Terminal` it returned Ok and macOS showed a '"Terminal" Notifications' permission prompt, so in `tauri dev` alerts are attributed to Terminal and gated on Terminal's permission. With `com.tryopendata.kelvo` from an unbundled binary, `set_application` failed (`CouldNotSet`: LaunchServices does not know the bundle) and posting still returned Ok, falling back to the library's default sender. An ad-hoc signed bundle has to have been launched (registered) once for its own identity to be used. Not observed: a bundled build's delivery, which needs the app installed.
- Clicks and actions are dropped on desktop. The supported route to open the Timeline at the event is `UNUserNotificationCenter` with a delegate that receives the response (for example through `objc2-user-notifications`), which is a new dependency and needs a signed bundle. Not done; the notification body says to open the Timeline, where the alert is a pill at its time.

### Revisit when

- tauri-plugin-notification gains desktop click handling or a real permission step.
- Kelvo is signed with a stable identity (Developer ID), which makes `UNUserNotificationCenter` and click routing worth adding.

## D-085: Per-process GPU reads the GPU's IORegistry user clients; shares of the whole GPU, clamped, approximate under long command buffers

Status: Accepted. Date: 2026-10-05. Phase 1.2-B. Amended after the v1.2 code review (bullets marked "amended after the v1.2 review").

### Context

v1.2 needs per-process GPU time for the Overview GPU card, the GPU page and the Processes page. The plan named IORegistry `accumulatedGPUTime` as unverified. A spike (M3 Max, macOS 27, unprivileged, ad-hoc signed) confirmed it:

- Every process that opens a Metal device gets an `AGXDeviceUserClient`, a child of the `IOAccelerator` service in the IOService plane. The clients are not registered, so `IOServiceGetMatchingServices` returns none; the accelerator's child iterator finds them (90 on the dev machine, 30 with usage).
- `IOUserClientCreator` is `"pid N, name"`, the name cut to 16 characters. `AppUsage` is one dictionary per command queue with `accumulatedGPUTime`, GPU nanoseconds, monotonic for the client's lifetime; an empty array for a client that never submitted. Exited processes' clients disappear; no counter resets were seen.
- The counter moves when a command buffer completes.

### Decision

- **Source.** `kelvo-collect/src/macos/gpu_procs.rs`, collector `gpu_per_process`, `Cadence::OnDemand(Interest::GpuProcesses)`, `Entitlement::IoRegistryGpuClients` (not in the `appstore` build). Plain registry reads through `iokit.rs`, which gains `children()` (child iterator, no allocation) and `registry_id()`. It probes `Supported` when an accelerator has at least one client that names its creator.
- **Measure.** Per pass: each client's summed `accumulatedGPUTime`, keyed on registry entry id (a process can hold several clients; WindowServer holds two). Per pid: the sum of its clients' deltas divided by wall time between samples, as percent of the whole GPU (all cores), the same scale as `gpu.util`. Every client that names its creator is recorded, idle ones included, so a client that starts submitting counts its first interval; once primed, a client absent from the previous pass counts from 0 (registry ids are not reused). A client whose counter went back adds nothing that sample, and one whose usage could not be read keeps its previous count. If the registry changes during the walk (`IOIteratorIsValid` false) the walk is read again once; if still cut short, clients it missed keep their counts for that one pass only (a second cut pass in a row keeps nothing and the next pass is a baseline), and a cut pass never primes a fresh ledger, so a missed client is never charged its lifetime. An accelerator whose child iterator cannot be created fails the pass (`CollectError::Os`) and the next sample starts from a baseline. The first sample after the interest starts is a baseline. (Amended after the v1.2 review.)
- **Clamp.** Each process is clamped to 100%, and when the shares add up to more than 100% all are scaled down together, so no view shows an impossible GPU.
- **Interest and lifecycle.** Like per-process network (D-082): `ProcessView.gpu` asks for it, the live registry ORs it over visible windows (`set_gpu_process_interest`), the engine samples the collector only on process ticks and calls `release`, which forgets the accelerators and counters, when the interest ends. A measured sample gives `0` to rows without GPU time; `null` means not measured. `ProcessSort` gains `gpu`; `Capabilities.process_gpu` says whether it is available.
- **Names** come from the process rows; the creator name is not used.
- **Approximate under long buffers, said in the UI.** The GPU page's process table notes that times are approximate while long compute jobs run. The Overview card has no note, to stay close to board 04.
- **Budgets.** `perf-budget.json` gains a `windowGpu` engine mode with an iokit ceiling of 350 calls per tick. The `trayOnly` and `window` iokit ceilings (6) leave no room for the walk, which makes them the zero-idle-cost check.

### Measurements

Dev M3 Max (40 GPU cores), macOS 27.

- One pass over 90 clients: 1.1 to 1.2 ms wall, 211 IOKit calls (one child iterator, two property reads per client, one registry id per client with usage), 0 Rust allocations.
- `perf_gates`: `trayOnly` and `window` 0 `gpu_per_process` samples; `windowGpu` 216 iokit calls per tick in all (277 after the v1.2 review fixes, which read the registry id of every client and check the iterator; ceiling 350), 0 collector allocations, engine core 9.02 allocations per tick, the same as `window`.
- Release `dump --perf`, five engines side by side for 120 s (load average 11 to 24 from other work on the machine), collector thread time: `gpu_per_process` 0.027% of a core with process rows every 5 s (20 samples), 0.148% at every tick (104 samples), 0 samples tray-only and without GPU interest. Whole engines: tray 0.241%, Overview-style window 0.663%, the same with GPU 0.700%, every-tick rows 0.958%, with GPU 1.139%.
- Live check against the Metal load generator's own measurement (sum of `gpuEndTime - gpuStartTime` over its command buffers, divided by wall time), an 8 s Kelvo window inside a 10 s run. Activity Monitor's GPU column could not be read non-interactively, so the load's self-measurement stands in for it:
  - 9 ms buffers back to back: Kelvo 96.6% for the load, load 97.4%; WindowServer 2.4%, ghostty 0.8%; sum 99.8%.
  - 9 ms buffers with 10 ms sleeps: Kelvo 39.8%, load 37.7%; sum 41.1%.
  - 0.64 ms buffers back to back: Kelvo 97.7%, load 76.7%. With buffers this short `accumulatedGPUTime` includes per-buffer time that `gpuEndTime - gpuStartTime` leaves out, so Kelvo reads higher than the load's own figure. Ordering is right.
  - 1.86 s buffers, 1 s window: Kelvo gave the load 0% and charged ghostty 83.2% and WindowServer 16.8% (clamped and scaled to 100%). The window ended before the load's buffer completed, and the clients queued behind it were charged its time. This is the long-buffer caveat.

### Consequences

- Shares are right for work in short command buffers (UI, video, games) and wrong, in both directions, for a single process submitting buffers seconds long. A longer sample period helps: the Overview samples every 5 s.
- The sandboxed build loses per-process GPU, as architecture.md said.

### Revisit when

- A macOS release changes the client class, the property names or the creator format: the probe then fails and the columns hide, the fallback this decision relies on.
- A source with per-interval GPU time per process (not per completed buffer) appears.

## D-086: One-shot motion choreography is allowed; motion tokens split by cost, and power saver stops only per-tick tweens

Status: Accepted. Date: 2026-10-05. Design (motion).

### Context

Motion covered continuity and feedback only: 150 ms bar, ring and chart-scroll tweens, the card hover glow, shadcn enter/exit. `motion.md` banned entrance staggers, and the entry and crossfade rows of design-system.md "Motion" were never built. The app read as static. The user asked for framer-style motion that delights without overwhelming, borrowed from the opendata frontend, with tray-only cost unchanged and foreground cost measured before any limit is set.

### Decision

- **One-shot choreography is allowed.** A dashboard page's sections lift in (fade plus 3 px rise, 280 ms, 30 ms apart, capped at 8 steps) on every navigation; Overview's cards one by one. Onboarding steps do the same. The sidebar selection is one pill that slides between rows. Buttons darken on press; segmented options and the switch thumb squeeze. State changes (status pill, pressure badge) fade in. Dialogs, menus and tooltips use a softer zoom (0.97) and dialogs rise. Per-tick motion is unchanged: numbers stay instant, charts keep the `translateX` scroll, nothing loops.
- **CSS only, no motion library.** The vocabulary comes from opendata (`/ask`'s keyframes and wrappers, the landing page's lift roles and capped stagger); the `motion` package, `will-change`, blur and scroll reveals are left out. Primitives live in `src/app/lib/motion/` (`pageEnter`, `data-stagger` + `stagger()`, `enter()`, `Swap`). Opendata's `ModuleEnter` and `Collapse` are not ported: nothing in Kelvo can toggle a module while its surface is visible, and both animate layout.
- **Tokens split by cost.** `--motion-tick` (150 ms, with the gentler `--ease-tick`) carries the per-tick tweens; `--motion-fast`, `--motion-entry`, `--motion-enter`, `--motion-stagger` and `--motion-crossfade` carry one-shot motion. `data-power-saver` zeros only `--motion-tick`: a one-shot animation has no idle cost, so it stays on battery. Reduced motion still zeros every token, with no JS branch.

### Measurements

`perf-gate.spec.ts` main-thread ms/s on the dev M3 Max, same sitting, main then this change: popover 18.9 / 20.7, Overview 16.9 / 15.2, CPU 1 h 21.4 / 18.6, no long tasks either way. The spread is the gate's usual run-to-run noise (D-060); this change adds no per-tick work. Whole-app (`make bench`) popover and Overview numbers are taken with the follow-up chart-motion change, which is the one that can move them.

### Cut after review

A devil's-advocate pass (product, design, UX, motion craft) cut a y-scale tween (it would draw the line against the wrong axis mid-tween), a chart draw-in clip (popover charts mount hidden, and it reveals "now" last), a History crossfade (keying remounts uPlot), sidebar item staggers, and press scale on buttons (under a pixel at 24 px; native buttons darken). A looping shimmer on the collecting state was also dropped: collecting lasts hours and nothing is loading.

### Revisit when

- The popover choreography lands: it replays on show, so it needs a replay policy and a check that WebKit doesn't paint a stale frame first.
- Chart-update motion (longer tick step, leading-edge marker) is measured; foreground limits are set from those numbers.

## D-087: Headline numbers count to their next value on large moves; parsed from the formatted text, log space on unit ladders, off in power saver

Status: Accepted. Date: 2026-10-05. Design (motion). Amends D-086, which kept every number instant.

### Context

The user asked for the common integer-change pattern: when a reading goes from 10 MB to 100 MB it counts up to 100 MB, and from 10 GB to 100 MB it counts down. Opendata has it twice: `shared/lib/tween.ts` (`createTween`, a rAF tween that retargets from its live value) and `talks/shared/components/RollingNumber.vue` (parse prefix, number and suffix out of the display string, ease-out cubic, write the exact final string on the last frame). Widget props carry formatted strings, and that prop contract is what the v3 WidgetKit feed exposes, so the ticker has to work from the string.

### Decision

- **`<NumberTicker text unit?>`** in `src/app/lib/motion/`, used for the ring centre value (`RingGauge`) and the stat-strip hero and items (`StatStrip`). Tables, process rows, bars, the sidebar and the popover's inline figures stay instant.
- **Parsed from the formatted text** (`parseFigure` in `@core/format/figure`). On the byte and rate ladders the figure is converted to base units, and in-between frames go back through the formatter's own `scale`, so the unit moves with the value ("10.0 GB", "1.0 GB", "316 MB", "100 MB"). Other figures keep their suffix, precision and grouping. A unit outside the text (a ring's "GB USED" label) is passed as `unit`; when it changes the number lands.
- **When it counts.** Only between two figures of the same kind that moved 10% or more or changed unit, and by at least three display steps (5% to 6% doesn't count). Missing values ("—"), a change of kind, and 0 on a ladder land in place. Ladder figures count in log space, so a drop of two orders of magnitude spends as long in each unit.
- **How.** A port of opendata's `createTween` in `src/core/tween.ts`; frames write the span's text directly, so a count never re-renders React. A new value mid-count retargets from the live value. The last frame writes the exact `text`.
- **Duration** is `--motion-count`, 450 ms (inside the 1 s tick with room for the next value), read from the root when a count starts. It is 0 under reduced motion and in power saver, like `--motion-tick`.
- **GPU.** Changing glyphs is text layout and paint on the main thread; the compositor can only move or fade layers that are already painted. A per-digit roll (`translateY` on digit columns) would composite, but it can't change digit count or unit and adds a column of elements per digit. The count stays on the CPU and is bounded by the 10% rule and the handful of instances per view.

### Measurements

`perf-gate.spec.ts` main-thread ms/s on the dev M3 Max, same sitting, `--motion-count` 0 versus 450 ms, three runs each, alternating: Overview 11.7, 16.0, 12.4 (median 12.4) versus 14.3, 13.8, 13.7 (median 13.8); CPU 1 h 15.5, 23.3, 17.0 (median 17.0) versus 21.1, 18.1, 17.5 (median 18.1); popover (no tickers) 9.2, 18.4, 11.9 versus 12.6, 9.4, 9.5. No long tasks. On the mock feed the Overview's six tickers made 564 text writes in 10 s against 192 with counting off. About 1.4 ms/s of main thread on the Overview, inside the gate's thresholds; whole-app `make bench` was not run for this change.

### Revisit when

- `make bench` Overview or popover moves with tickers on real data, or the ticker spreads to more than a handful of figures per view.
## D-089: Per-process network history: always-on per-app bytes, persisted, with a remainder split

Status: Accepted. Date: 2026-10-05. Collect, engine, store, shell and Network page (network attribution, after v1.2). Amends D-081 and D-082.

### Context

The Network page shows interface throughput and live per-process rates, but once a spike passes nothing says what caused it, and "what used the most bandwidth in the last hour" has no answer. The feature: select a range on the throughput chart (or pick 1m/5m/15m/1h) and list the apps that moved the most bytes in it. That needs three things D-081 and D-082 ruled out. NetworkStatistics has to run with no window open, so the bytes exist when someone asks later. Per-process bytes have to be stored. And the list has to add up to the interface total, so the "System and other" remainder D-082 deferred is now required.

Before building, two spikes checked the cost of always-on sampling and whether an app identity can be resolved reliably. A third check measured how NStat bytes relate to interface bytes.

### Spike

Dev M-series Mac, macOS 27, unprivileged, ad-hoc signed. Artifacts are in the session scratchpad (`identity-probe/`, `nstat-cost-spike.diff`, bench `out/`), not in the repo.

**Cost.** The spike build kept one NStat manager open tray-only and queried it every 10 s. 13 alternating 120 s tray-only coalition runs on battery, base build against spike build:

| | Base | Spike |
|---|---|---|
| Whole app, coalition CPU | 0.647% (sd 0.075, n 6) | 0.729% (sd 0.111, n 7) |
| GCD callback threads (unnamed, where `kelvo.nstat` runs) | 0.028% | 0.092% |

- The whole-app delta (+0.08 pp) is within the noise.
- The GCD thread rise is real: +0.065 to +0.10 pp. The 10 s query itself costs about 0.0016%. The rest is the open manager's own callbacks, so the cost follows how long the manager stays open and how often flows churn, not the query cadence.
- Wakeups were inconclusive: too few clean pairs.

**Identity.** An identity probe resolved each flow's owner to an app name and checked the result against what a user would call the app. The rules that held, in order:

| Case | Rule | Example |
|---|---|---|
| Path contains `.xpc/` | Use `responsible_pid`'s app. Only for `.xpc/`: every CLI's responsible pid is the terminal (Ghostty), so applying it more widely would charge all CLI traffic to the terminal | `com.apple.WebKit.Networking` reads as Safari |
| Path inside an `.app` | Outermost `.app` name, unless the path runs through `.app/Contents/Developer/` (Xcode toolchain) or `Python*.framework/`, which fall through | "Google Chrome Helper" reads as Google Chrome; Xcode-shim `git` and `/usr/bin/python3` do not read as Xcode |
| Otherwise | argv[0] from `KERN_PROCARGS2`, cut at the first whitespace, basename | `npm exec cowsay hi` reads as `npm` |
| argv unreadable | `name_of` | |
| Then | Alias table: `git-remote-http`, `git-remote-https` and anything under `/git-core/` become `git` | |

- Capture point: NStat delivers an unsolicited counts callback carrying the pid about 1.5 to 3 s after a flow opens, usually while the pid is alive. Resolving there and caching by `uniqueProcessID` (a description key) avoids pid reuse.
- When the pid has already exited, the counts dictionary's `processName` is the fallback. It was present in 100% of 8,116 observed callbacks. An empty name goes to "other apps".

**Header overhead.** NStat counts TCP payload; interface counters include L2 to L4 headers. For an 80 MB `curl` download, interface bytes over NStat bytes measured 1.0465 against a theoretical 1.0456. The ACKs for that download put about 0.44 MB on interface tx that no app shows. Interface packet counts are exact, so overhead can be estimated as interface packets times a per-packet header size.

**Interface counters.** For unprivileged callers, `NET_RT_IFLIST2` byte counters are 32-bit and 1 KiB-granular, despite the `if_data64` struct. Packet counts are exact. `network.rs` already handles the wrap; its comment saying `if_data64` avoids it is wrong.

### Decision

- **Pass criterion.** Over 6 or more alternating tray-only pairs against the base build: GCD plus `kelvo-engine` thread time rises at most 0.10 pp, whole-app coalition CPU rises at most 0.10 pp, and main-process wakeups rise at most 0.5/s. Passes with a footprint over 150 MB are dropped (a window was showing). `perf-budget.json`'s `trayOnly` numbers are ratcheted to the result. The wakeup check needs 4 or more clean pairs on AC, hands off; it runs in final verification and does not block the build.
- **Always on, aligned to process samples** (amends D-081, D-082). With Network history on, NetworkStatistics samples on the ticks where the processes collector sampled, ordered after it in the slot list: every 10 s tray-only, every tick while a process view is visible. `wants_network` is not changed for this, since `Cadence::OnDemand` gives period 0 and would either never sample or sample every tick. One manager stays open across view changes and is released on sleep, on pause and when the setting goes off. Tray-only NetworkStatistics calls are no longer zero; `trayOnly.nstat` gets a budget.
- **Per-sample bytes.** `ProcessNet` carries `identity`, `rx_bytes`, `tx_bytes` and the sample's `[prev_ns, now_ns)` interval beside the rates. The closed-flow accumulator keys bytes by identity, not bare pid, so a process that exits between samples keeps its name. Identity length is capped, and so is the number of new names per hour (overflow goes to "other apps"), so argv-rewriting processes cannot grow `proc_names` without bound.
- **Identity rule and capture point** as in the spike table above: resolved in `libproc.rs` at the first counts callback that names the pid, cached by `uniqueProcessID`, falling back to `processName` (read through `dlsym` like the other keys).
- **Persisted per-app bytes, an exception to "series, not typed structs".** Per-app bytes are stored as typed blob rows in `proc_net_10s`, `proc_net_1m` and `proc_net_15m`, following the `proc_snap` precedent: app names are unbounded and short-lived, which does not fit interned series. Each blob has a header (`measured_ms`, interface rx/tx bytes, rx/tx packets for the bucket) and rows of `name_id u32, rx u64, tx u64`; `name_id` 0 is reserved for "other apps". Each bucket keeps the top 20 apps by rx+tx and folds the rest into "other apps", so sums stay exact through rollups. 10 s rows are kept 72 hours, 1 m rows 7 days, 15 m rows for the history setting. 1 m is written live beside 10 s, so it has no 72-hour hole. The interface totals in the header keep the remainder independent of the tiers' min/max/avg resolution. Sizes and DDL are in architecture.md.
- **The remainder is split in two** (amends D-082's "no remainder row"). "Protocol overhead (est.)" is interface packets times a per-packet header constant (66 B for Ethernet, IPv4 and TCP with timestamps; Wi-Fi framing to be confirmed in tests), clamped to the interface bytes. "System and other" is interface bytes minus apps minus overhead, clamped at 0, with a `clamped` flag when the clamp hits. The Network page shows both rows and says why "System and other" exists.
- **Per-app bytes count only flows on a reported interface** (amended 2026-10-07). A flow counts when its `interface` index is Wi-Fi, Ethernet or cellular by the network collector's own rule, so apps and the interface split the same traffic. Before this only `ifLoopback` was checked: flows on VPN tunnels, VM bridges and AWDL, and one end of some 127.0.0.1 connections, were charged to apps but never counted on the interface, and the Apps table showed shares past 1,500%. The type flags cannot replace the index: a Tailscale flow on `utun4` reports `ifWiFi`. Shares are of the table's total, which is the interface total except while `clamped`.
- **"Network history" setting**, default on. The user accepted the cost above. Off releases the manager when no view asks, and per-process rates go back to D-082's on-demand behaviour.
- **The appstore build hides the feature,** as it already drops `Entitlement::NetworkStatistics`.
- **Per-app data is not synced.** Proc tables are not part of the sync protocol, and `proc_net_*` is not either.

### Consequences

- Root daemons are still invisible (D-081): mDNSResponder, software update and root VPN tunnels land in "System and other". Attributing them needs a privileged helper.
- Protocol overhead is an estimate from a fixed per-packet header size. Other link framing, IPv6 and UDP all skew it, which is why it is labelled "est.".
- Tray-only CPU rises by about 0.065 to 0.10 pp. The user accepted this; performance mode and the budget policy are being decided separately.
- Interface bytes come from 32-bit, 1 KiB-granular counters, so a bucket's interface total is accurate to about 1 KiB per sample.
- A process that exits before its first counts callback is named by the fallback, NetworkStatistics' `processName`, not by the identity rule, so a short-lived helper's bytes can land in a second row under its own process name instead of its app's. Accepted: it needs a process that lives less than the 1.5 to 3 s before that callback, and those move few bytes.
- **Buckets are filled pro rata, not phase-locked** (engine wave). The network collector runs every tick while shown and every 10 s otherwise, on its own phase; per-app samples follow the process ticks. Each sample's bytes are split across the 10 s buckets its interval overlaps, apps and interface the same way, using floored cumulative shares so the pieces add up exactly. A bucket closes when both streams have reported past its end, or 3 × max(base tick, 10 s) after it. Phase-locking would have needed a new cadence kind in the collector crate and still broke on a skipped tick. In a partly measured bucket (a baseline, a reset) the interface total covers more of the bucket than `measured_ms` does. Until a bucket closes it holds the streams' bytes over different spans, so `query_network_by_app` returns `complete_to_ms` (where the open buckets start): the Apps card's window and "Now" end there, and an answer is cached only once it is complete. A reset other than a clock step (history or the network module toggled) keeps the edge the written buckets reached, so a bucket is never reopened and its row overwritten with a part of it.
- **Bytes moved while a flow's owner is unknown** (amends D-082 for history only). D-082 drops what a flow moved before a description names its owner, so the rate doesn't spike. That stays true for rates. For a flow opened after the baseline, those bytes now go to the owner's byte totals, and so to history, in the sample where the owner becomes known. They travel on `ProcessNet` as separate late bytes, kept out of that sample's rate. The engine adds them whole to the oldest open bucket that has measured time, so they never reopen a closed one. The cost is a shift of up to about two buckets: the wait for a description (up to 10 s) plus the bucket boundary. Late bytes are also dropped whenever the sample's span is empty. That happens when the sample falls wholly before a restart. It also happens for samples clipped by the closed edge: in the first 10 s or so after startup when the previous run's written bucket ends in the future, and after history or the network module comes back on within the bucket the reset wrote. Both cases are rare, since they need a flow whose first counts lack an owner. Live verification found the old rule sent 18 to 33% of a 30 s curl download to "System and other". Flows that were already open at the baseline still drop the wait, because those bytes may predate it.
- **Budgets** (engine wave, raised for this decision). `perf-budget.json`: `trayOnly.nstat` 0 → 0.25 (measured 0.10); `window` and `windowGpu` nstat 0 → 1.2 (every process sample while a view shows processes); a new `trayOnlyHistoryOff` mode keeps D-082's zero. `store.rowsPerHour` 900 → 1,400 (measured 1,188, from 774) and `store.walBytesPerHour` 5.0 → 6.0 MB (measured 4.8 MB, from 4.0). Engine-core allocations per tick, tray-only, 7.22 → 7.47, inside the existing ceiling of 10.

### Revisit when

- Kelvo gains a privileged helper (v3+): root daemons can then be attributed, and "System and other" shrinks to what is really unknown.
- A macOS release renames or removes the NetworkStatistics keys, `uniqueProcessID` or `processName`, or changes when the unsolicited counts callback arrives.
- The pass criterion fails in final verification: the cadence or the always-open manager is the first thing to change.

## D-090: The live stream publishes each series' kind and how long a sample stays current; charts join and fill samples from those facts

Status: Accepted. Date: 2026-10-06. Live channel (IPC), catalog, engine ring. Follows the data-boundary rule (`.claude/rules/data-boundary.md`).

### Context

The Power page's component stack showed isolated dots across most of its 10 minutes. The data was complete: every 10 s bucket held `power.cpu` and `power.gpu`. `power.gpu` comes from the IOReport collector, which samples every 10 s while only the tray is open (D-061), and `power.cpu` from the SMC every tick (D-070). In the 1 Hz ring, GPU had a value on one row in ten, the stack needs every component in a slot, and each lone slot drew as a dot. The CPU, network and disk collectors have the same tray-only cadence, so their charts break the same way over history recorded with no window open.

The client could not tell a slower sample from a gap, because the live stream never said how often a series was sampled, nor what a value means. A counter delta (CPU load, residency, energy watts, byte rates) is the average over the span since the series' previous sample; a gauge is a reading at its instant. The catalog called most counter-derived series `Gauge`. The client guessed instead: `GAP_FACTOR` copied the ring's segment rule, and a draft fix copied the engine's 2.5x staleness and the 10 s tray-only period into TypeScript.

### Decision

- **`MetricKind::Mean`**: the mean over the span since the series' previous sample. CPU load and its user/system split, cluster frequency, active residency, residency and power, GPU frequency and residency, and the energy-derived power series (`power.cpu`, `power.gpu`, `power.ane`, `power.dram`, `power.package`) are `Mean`. `Rate` is redefined the same way: a per-second rate over the span since the previous sample. Gauges stay `Gauge`. `power.cpu` and `cpu.cluster.power` from the SMC (D-054) are readings, but they are sampled every tick and calibrated to PMP energy, so `Mean` describes them over any span a chart sees.
- **`hold_ms` per series.** Every `LiveFrame` carries how long each series' sample stays current: 2.5 times its sampling period as it is on that tick (the rule `held` already uses, D-047). The frame shares one `Arc` across ticks until a period changes, so steady state does not allocate. Ring rows keep it, and a backfill segment ends where it changes.
- **The live channel sends the facts.** `Layout` carries `kinds`, parallel to `series`. `Backfill` and `BackfillEarlier` carry `holds_ms` for their segment. A new `Holds { layout_no, holds_ms }` message precedes the first frame whose holds differ from the last ones sent on the channel. Series picks apply to both.
- **Charts join samples by these facts.** Two consecutive samples of a series belong to one run when the later one is no more than the earlier one's `hold_ms` after it; otherwise there is a gap between them. Inside a run, the empty slots between them take the later value for `Mean` and `Rate` series (that value is the average over those seconds) and the straight line between them for gauges (the segment the chart draws anyway). This is display only: ring rows, rollups and the store keep measurements alone. `GAP_FACTOR` and the per-row spacing column go.
- **The catalog's kinds are exported to TypeScript** as a generated constant, so the mock transport labels its layouts from the real catalog. The mock models the tray-only 10 s cadence of the adaptive collectors in the history it backfills, and `held` going stale.

### Consequences

- The power stack, and the CPU, network and disk charts, read continuously over history recorded with only the tray open, at the resolution it was sampled at.
- One missed tick is joined across, as `held` already tolerates one missed sample; two are a gap.
- `LiveMsg` changes shape (IPC between the engine and its own webviews, which ship together; not the sync protocol).
- Window statistics that average samples (`series-stats.ts`, the popover average) can now weight by span; they are not changed here.

## D-091: One chart window for every module page, saved as a setting

Status: Accepted. Date: 2026-10-06. Settings, dashboard module pages. Amends D-061 (window controls following the interval).

### Context

Each module page kept its own 1m/5m/15m/1h choice in component state. Switching tabs reset it, and nothing saved it. Several cards on the same page ignored it. With 15m picked on CPU, "Per-core load, last 10 minutes" sat under a 15-minute chart. The GPU power, swap and power stack cards were fixed at 10 minutes, and GPU frequency at 60 s.

### Decision

- One window, `general.chart_window`, owned by Rust like every other setting (D-050). It is shared by every dashboard window through `settings-changed` and survives restarts.
- The options are 5m, 15m, 30m and 1h, and the default is 15m. 1m is gone: the popover is the 60 s live glance. All four windows fit in the one-hour ring, so nothing reads SQLite.
- The selector appears on CPU, GPU, Memory, Power (new), Network and Disk. Every time-based chart on those pages follows it, with titles, aria labels and ticks to match:
  - the per-core heatmap: about 60 columns, with buckets of 5, 15, 30 or 60 s, raised to at least the sampling interval
  - GPU power and frequency residency
  - swap
  - the power stack and its annotations
  - zone min/max
  - the network mirror chart, Apps table and selection note
  - the disk mirror chart
- Unchanged:
  - Overview and the popover keep their fixed, interval-scaled 60 s glance.
  - Timeline keeps its Range control.
  - Ceiling-only reads (the Power ring, Battery) and the Settings overhead stat show no duration.
- Slow sampling: a window that would hold fewer than 10 samples is still disabled (D-061). The page shows the shortest allowed window, but the saved choice is not rewritten, and when the interval speeds back up the saved window returns. Today that only affects 5m, at 60 s sampling or 30 s backed off on battery.
- `useChartWindow` is null until settings load, so a saved 1h never renders as 15m first and the Apps table doesn't query the wrong span.

### Consequences

- Board 07 (1m pressed, no 30m) and board 16 (5m pressed) are out of date on the options and the default. This is a deliberate change.
- GPU frequency residency and zone min/max now scan the chosen window on every tick (`useRingStats`) instead of 60 s and 10 minutes. `perf-gate.spec.ts` measures GPU and Power at 1h.

### Revisit when

- Someone wants a window longer than an hour on a module page. That reads history, not the ring, and probably belongs in Timeline.
- A per-card window turns out to be needed. The setting would then become a default rather than the only value.

## D-092: Rust publishes the remaining data facts: engine constants, process refusal, totals, host facts and span-weighted history

Status: Accepted. Date: 2026-10-06. Generated bindings, live channel (IPC), catalog, HostInfo, history queries. Follows D-090 and the data-boundary rule (`.claude/rules/data-boundary.md`).

### Context

D-090 moved sampling periods and metric kinds into Rust. An audit of `src/core` and `src/app` then found about 40 more places where TypeScript decides what data means: engine constants copied by hand, aggregates computed per component, and the timeline rolling up the live ring itself. Most agree with Rust today and will drift. Some already disagree: `refusalReason` misses WebKit helpers, negative pids and Kelvo's own pid; `diskTotal` treats a missing side as 0 while the mock's sort uses a third rule; memory fractions use two different denominators; network totals go null in the popover but sum present parts in the tray; the timeline sums per-interface min/max into an envelope wider than the true total's; rollups average by sample count, so a minute where a collector moves between 10 s and 1 s leans toward the 1 s samples; `self.cpu` is catalogued `Gauge` but is a span mean.

What stays in the client is view shaping and transport facts: stacking and stack remainders (null when any part is missing), grouping parts on one card, fan max across fans, channel liveness in `frame_period_ms`, unit conversions, and picking a history tier and bucket width for a chart's pixel width.

### Decision

1. **Engine and settings facts are generated constants**, exported next to `METRIC_KINDS`, and the TS copies (the mock's included) go: ring span and rows, network bucket and header constants, the hold factor, the settings domains (intervals, retention, size limits, menu-bar modes, modules), history tiers, the history projection's model inputs, metric units, metric codes (real enums in `kelvo-schema`) and `SAMPLING_PLANS`: a table over the finite settings domain built from the engine's `effective_interval`, the live window floor and the tray pacer, which the settings panel's hypotheticals read.
2. **Process refusal comes from Rust.** `LiveProcess.refusal` is an enum set by the same predicate `signal_process` checks; Kelvo membership comes from the self-CPU coalition the engine already caches. `signal_process` keeps its own checks. `diskTotal` and the mock's sort follow `procview.rs`.
3. **Network and disk totals are catalog metrics** (`net.rx_total`, `net.tx_total`, `disk.read_total`, `disk.write_total`), summed in the collector over the parts it reports on that sample, and a gap when any part is a gap. They replace the timeline's summed envelope and every client and tray sum. The per-app remainder keeps its partial rule.
4. **Host and power facts.** `HostInfo` gains `gpu_dvfs_mhz` and `boot_mounts` (serde default, skew fixtures refreshed); `LiveStatus` gains `primary_iface` and `power_source`.
5. **Rollups are span-weighted and history answers through now.** The writer weights `Mean` and `Rate` samples by their span, capped at the hold; the engine merges its uncommitted bucket rows into `query_history`; `HistorySeries.hold_ms` tells the client how far to join points. The client drops its own ring rollup and stitching. `battery_hours` takes the client's hour boundaries.
6. **Live window statistics use the display grid**, which makes them span-weighted without new data. `self.cpu` becomes `Mean`.
7. **Smaller fixes**: one memory denominator (`mem_total_bytes`), the disk card reads `disk.used`, and the mock's `held` goes stale so the read-failure UI can be exercised.

The work lands in three groups in order: generated facts and refusal (1, 2), totals and host facts (3, 4, 7), history and statistics (5, 6).

### Consequences

- Each fact has one definition, in Rust. A changed engine constant reaches the UI and the mock through `make bindings`, and CI's freshness check catches a stale copy.
- Four new persisted series and `self.cpu` → `Mean` add about 3% to store payload; `engine.store.payloadBytesPerHour` is measured, and raising it needs its own entry.
- IPC grows (`LiveStatus` +2, `LiveProcess` +1, `HistorySeries.hold_ms`, a `battery_hours` command). `HostInfo` is additive on the proto `Hello`.
- Rows written before span weighting stay count-weighted. Acceptable pre-v1.
- The tray-only allocations-per-tick gate must hold after each group.

### Revisit when

- A consumer outside the webviews (WidgetKit, a remote controller) needs a generated constant: it then belongs on the wire, not only in the TS bindings.

## D-093: Energy by app over the chart window; addresses on Network; brush dismissal; °F by default

Status: Accepted. Date: 2026-10-07. Power & Sensors, Network and Settings pages; `kelvo-collect` process samples, `kelvo-engine` live hub, IPC commands.

### Context

The Power & Sensors page shows what draws power now but not what drained the battery over the last half hour, which is the question a user with a dropping battery asks. The Processes page answers "what is busy now"; it can't answer "what used the most energy over a window" because processes come and go and a busy-now ranking forgets them. On Network, users asked for the machine's address and had no way to drop a brushed range except a "Clear" text button. Users in the US read temperatures in °F, and the first-run default was °C.

### Decision

1. **Per-process energy rides the process sample.** `ProcessSample` gains `energy_j` (CPU energy over the interval from `ri_energy_nj`), `app` (the D-089 identity, resolved once per process at first sight) and `app_main` (the outer bundle's own executable).
2. **The live hub keeps an hour of energy in memory.** `EnergyRing` merges `BusMsg::Processes` into 10 s buckets (`ENERGY_BUCKET_MS`, generated) for 1 h + one bucket, keyed by `(pid, start_time)`. Rows under 10 µW average are skipped; the floor is on power so it means the same at every cadence. It is not persisted: per-process energy in SQLite would cost rows per process per bucket, and the page's longest chart window is an hour. A clock step back drops buckets after it (a process with energy on both sides keeps what came before), and clearing history resets it.
3. **`query_energy_by_app(from, to)` answers over the page's chart window** (5m to 1h), grouped by app with each process listed, sorted by joules. It reports `since_ms` so the UI says when counting started. Processes with no app identity are their own app. Refusal is set only for running processes.
4. **The quit target for an app row** is its running main process, else its only running process, else none; the client matches it among running processes, since a reused pid can also list an exited one. Each running child keeps its own Quit.
5. **Coverage is stated in the table**: CPU energy as macOS estimates it, the user's own processes only. GPU, display and other users' daemons are not attributed, so totals undercount system draw.
6. **Addresses.** `get_network_addresses` reads the primary interface's IPv4 and non-link-local IPv6 with `getifaddrs`. `get_public_ip` asks `https://api.ipify.org` through `/usr/bin/curl` with a 5 s timeout, only while the Network page is open and only after the local read succeeds (so a remote host never shows this Mac's address), asked again every 10 minutes and whenever the primary interface or its local address changes. This is Kelvo's only outbound request besides the update check. Clicking an address copies it and the tooltip says "Copied".
7. **Brush dismissal follows d3-brush and chart tools**: a plain click on the chart with a selection clears it (a drag replaces it), Esc clears from anywhere except a text field or open dialog, and a press on empty space outside the chart and the views that read the selection clears it. The chip's "Clear" text becomes an × with the same name for assistive tech.
8. **`TemperatureUnit::Fahrenheit` is the default for a new install.** Existing `settings.json` files keep their value.

### Consequences

- IPC grows: three commands, `ENERGY_BUCKET_MS`, and three fields on `ProcessSample` (in-process only, not on the wire).
- Tray-only cost: one identity resolution per new process (argv is read only for processes outside an app bundle) and a hash-map merge per process sample (every 10 s when no process view is open). The engine gate's fake processes now carry energy, so the gate covers the ring: tray-only 8.8 and window 10.6 allocations per tick, under the 10 and 12 ceilings. Whole-app CPU was not measured with `make bench` for this change.
- Energy history starts at app launch and is lost on quit; the table says so.
- The public IP leaves the machine as a request to a third party. A privacy setting to turn it off is a candidate if users ask.

### Revisit when

- Users want energy by app over 24 h or longer: that needs a stored per-app rollup, not a longer ring.
- macOS exposes per-process GPU energy: add it to `energy_j` and drop the GPU caveat.


## D-094: The backgrounded app ticks at 2 s, samples temperatures every 10 s and processes every 30 s, and redraws the menu bar every 4 s

Status: Accepted. Date: 2026-10-07. `kelvo-engine` tick choice and cadences, `src-tauri` live stream and tray pacer, sampling-plan facts, mock transport, bench scripts.

### Context

Benchmarks on 2026-10-07 put the tray-only app at 1.21 to 1.38% CPU against the 0.5% product budget (D-088 scopes that budget to the backgrounded app). About half is the main thread redrawing the status items (11 to 14 ms of AppKit work per drawn frame at 26 frames a minute) and half the engine, led by processes, HID and SMC temperatures and SMC power. Performance mode already measured 0.985% with a 4 s redraw and 30 s processes, so most of what it saves was available without asking the user to turn anything on.

### Decision

1. **Backgrounded means no window shows detail the tray does not** (detail interest is zero, D-061): menu bar items only, no popover, dashboard or onboarding window. `EngineStatus.backgrounded` publishes it. It is a flag of its own, not "the tick is 2 s", because the tray needs it when the user's interval is already 2 s or longer.
2. **The base tick is at least 2 s while backgrounded** (`BACKGROUND_TICK_MS`, generated). It replaces the battery back-off rather than stacking on it, so on battery the backgrounded tick stays 2 s as before. `backed_off` compares the visible interval against the setting and stays false for it. The 2 s tick keeps every-tick SMC power inside the CPU-power calibrator's 2.5 s step limit.
3. **Temperatures every 10 s and processes every 30 s while backgrounded**, the slow periods Performance mode used (D-088). The menu-bar temperature exemption is gone: a temperature in the menu bar also updates every 10 s. Performance mode still slows processes with a window open; it no longer changes anything in the background. The hot-process detector weights its batches by the 30 s period.
4. **The menu bar redraws every 4 s while backgrounded and every 2 s with a window open.** With a window open it doubles when backed off or in Performance mode, as before. In the background it stays 4 s on battery too, since the background tick replaced the back-off instead of doubling; a slower tick spaces the frames out by itself. The tray reads `backgrounded` from the status.
5. **A window never sees the background's tick.** Detail interest crossing between zero and one sends the engine a command; it re-chooses the tick and publishes the status before acknowledging, and the live stream waits for that (at most 200 ms) before it resumes a window, and stops before the window's interest is dropped. When the tick gets faster on open, the engine samples at once if the last frame is a full tick old and the next boundary is more than half a tick away, so the popover's numbers are not up to 2 s stale. History holds cover the 2 s tick (the floor is the background tick after any back-off), so 2 s rows in a backfill are not read as gaps.
6. **The sampling plan facts follow**: the menu bar figure is the background pacer, background processes and temperatures are always 30 s and 10 s, and `temp_in_menu_bar` is dropped. The mock transport writes 2 s rows while the window is hidden and sends no status to a hidden window.

### Consequences

- Menu bar numbers update every 4 s with no window open. The tray sparkline keeps one point per engine frame, so it spans 40 s backgrounded and 20 s with a window open.
- Top-process snapshots land in one 10 s bucket in three while backgrounded, a hot-process alert can arrive up to 30 s later, and energy by app (D-093) is off by up to 30 s at a window's edges.
- Store rows per hour fall (measured 936 rows/h and 4.3 MB/h of WAL in the engine gate, from 1,188 and 4.8).
- `perf_gates` tray-only modes run at the 2 s tick, so calls and allocations are per 2 s. `trayOnlyPerformance` is merged into `trayOnly`. The ceilings for smc, hid, libproc and nstat went down, set between the measurement and the same run without the slow cadences. `allocsPerTick` for battery and disk_capacity went up from 0.5 to 1: the same allocations per sample, but a 2 s tick sees twice the samples of the 5 s and 60 s collectors (measured 0.60 and 0.80).
- `make perf` gates the engine at the tick it reports (`dump --perf` prints `tick_ms`), 2 s with no window. `make bench-perf-mode` runs only `overview` by default; its tray cases stay available by name and should save about nothing.
- Projected whole-app tray-only CPU is about 0.8%, still over 0.5%; the remaining main-thread redraw cost is the next lever (the subview spike). Not yet measured with `make bench`.

### Revisit when

- Users notice the 4 s menu bar or the slower temperature in it: a temperature-only item could keep a faster period.
- The tray draws through a subview and a frame costs a fraction of today's: the 4 s redraw may come back down.

## D-095: Public release prep: Apache-2.0, bundle identifier com.tryopendata.kelvo, design mocks retired

Status: Accepted. Date: 2026-10-07. License, bundle identifier, repository contents, plan docs.

### Context

The repo is about to go public. It had no license, the bundle identifier still named a personal namespace (`com.riley.kelvo`, D-027), and the repo held the original design mock boards that early version docs pointed at screen by screen. The app has since moved past those mocks, and checking new work against them had become a comparison with a design nobody intends to ship.

### Decision

1. **License: Apache-2.0.** It carries an explicit patent grant, and it is compatible with the vendored macmon code (MIT) and the bundled JetBrains Mono fonts (OFL).
2. **Bundle identifier: `com.tryopendata.kelvo`**, changed before any public release. The identifier fixes the Application Support and Logs paths, so history and settings from builds under `com.riley.kelvo` are not migrated. Kelvo is pre-v1 and has no migration paths.
3. **Design mocks retired.** The mock boards are removed from the repo and its history. The UI as built and [design-system.md](design-system.md) are the visual reference; new screens follow design-system.md and stay consistent with the screens that exist. Plan docs no longer point at mock boards; past comparisons stay in PROGRESS.md as records.

### Consequences

- Anyone who ran a build under the old identifier keeps an orphaned `~/Library/Application Support/com.riley.kelvo/` and its logs folder, and, if Open at Login was on, a stale entry in System Settings > General > Login Items. Deleting them by hand is safe. macOS also asks for notification permission again under the new identifier.
- "Done" for a UI task no longer includes a side-by-side with a mock screenshot. Visual review checks design-system.md rules and consistency with nearby screens.
- Specs for unbuilt v2+ screens that lived only in a board are kept as prose in their version docs.

### Revisit when

- A screen needs a new visual direction that design-system.md does not cover: write it into design-system.md first.

## D-096: macOS checks move to a pre-push hook; hosted CI runs Linux jobs on push

Status: Accepted. Date: 2026-10-07. CI, developer hooks.

### Context

Every push ran two hosted macOS jobs of about 12 minutes each (`macos-26` and the `xcode-27` preview). On a private repo macOS minutes bill at 10x, so each run cost about 240 of the Free plan's 2,000 monthly minutes. Sixteen runs in three days used the whole allowance (194 macOS and 103 Linux minutes, 2,043 billed), and GitHub stopped starting jobs. The org has one self-hosted Mac (m4-mini), but it is registered to another repo, runs that repo's CI one job at a time, and a self-hosted runner on a public repo runs fork PRs' code on that machine.

### Decision

1. **A pre-push hook runs the macOS checks**: `make check bindings-check e2e-perf` (fmt, clippy, the appstore edition, `cargo test --workspace` with the engine perf gates, Biome, typecheck, Vitest, the bindings freshness diff, and the Playwright frontend perf gates). It is a `pre-push` stage hook in `.pre-commit-config.yaml`, not a second hook manager, and it skips pushes that touch no code or manifests. `make hooks` installs it.
2. **Push and pull_request run Linux jobs only**: a new `frontend` job (Biome, typecheck, Vitest, Playwright functional specs on the mock transport), the portable crates, and the dependency audit.
3. **The macos job stays in the workflow but runs only on a manual dispatch**, for release checks and anything the hook cannot see (the Tauri debug build and `make check-deps`).
4. **The Playwright perf gates run only on macOS.** Their budgets are macOS Chromium numbers. In a Linux container on 2026-10-07 every functional spec passed (215) while the perf gates failed or flaked (5 of 8), so they say nothing on Linux.

### Consequences

- Nothing checks the macOS-only code (collectors, src-tauri, the bindings) for a contributor who skips the hook (`--no-verify`) or has not installed it, or for an outside PR, until someone dispatches the macos job.
- A push of code takes several minutes longer on the developer's Mac.
- The Tauri build and the runtime dependency check (D-058) no longer run on every push.

### Revisit when

- The repo is public: hosted runners are then free, and running the macos job on push and pull_request again (or at least on pull_request) restores the PR check at no cost.
- Outside contributions start: a PR check that does not depend on the author's hooks matters more than it does with one developer.

## D-097: The live ring starts from stored history after a restart

Status: Accepted. Date: 2026-10-07. Amends D-066.

### Context

Live charts and the per-core heatmap draw from the host's in-memory ring (D-066). After a restart the ring was empty, so a 1-hour live chart showed only the seconds since launch while the store held the hour before it. The Timeline showed that history; the module pages did not.

### Decision

- `LiveHub::warm` fills an empty ring from a store read of the 10 s tier over the ring's span (`RING_SPAN_MS`). One row per bucket holds each series' bucket average, at the bucket's end, since a mean or rate covers the span before its sample (D-090). Each series is held for the hold of the slower of 10 s and its catalog period, so consecutive buckets join and a missing bucket is a hole.
- The rows carry their own layout, `WARM_LAYOUT_NO` (`u32::MAX`), which never collides with a source's layouts (numbered from 0 in a run), on timeline 0. They reach windows through the existing `Backfill` and `backfill_earlier` messages, so the wire format does not change. The rows age out of the ring as frames arrive.
- The shell warms the local host's hub at startup, before the source starts. A ring that already has rows is never warmed. A failed read leaves it empty, as before.

### Consequences

- The pre-launch part of a live chart has 10 s resolution and loses each bucket's min and max. The oldest stored sample covers only its own slot. The time Kelvo was not running stays a hole.
- A v4 remote source can warm its hub the same way from synced history.
