# Native defaults and compact popover execution

Implementation lane: `codex/ux-native-popover`, isolated checkout. Implements sections
1 and 4 of product-ux-polish.md. Integration, final performance gates and landing remain
orchestrator-owned. No performance budgets or history caps changed in this lane.

## Implemented behavior

- First-run/invalid-settings temperature defaults come from public Foundation measurement
  formatting. Valid saved Celsius/Fahrenheit choices survive. No new temperature enum.
- Battery's effective module state controls its Power-page history section. Missing and
  not-present capabilities redirect a direct Battery route only after capabilities load.
  Existing remote/local module semantics and useful system draw information remain.
- Memory Pressure/Used and Swap are stacked at full width for Mac and Linux.
- Gap presentation retains reason and original closed duration before clipping. Closed
  pauses below 5,000 ms and open pauses suppress prominent captions without changing
  masks, recorded gaps or line breaks. Other reasons and the exact five-second boundary
  retain captions. Pause captions use plain text and a muted glyph. Existing brush
  targets expose the explanation through hover titles and keyboard value descriptions;
  mirrored-chart tooltips also explain pauses. Labels do not intercept brushing.
- Rust-owned `popover` settings store seven independent expanded/compact fields.
  Partial writes preserve other preferences. Collapse/Expand all includes hidden cards;
  combined Power/Battery uses Power's choice; standalone Battery keeps its own choice.
- Each card renders one presentation. Compact CPU/GPU/Power/Network retain small existing
  chart primitives; Memory/Battery/Containers retain meters and meaningful status.
  Sensor failures, missing values and Docker partial/disconnected states stay visible.
- Separate chevron controls preserve dashboard navigation and keyboard focus. Header
  options provide Collapse all, Expand all and Open settings. Save failures preserve
  the prior presentation and offer a single Retry notice that clears on success.
- The shell measures header, intrinsic card content and footer independently of viewport
  height. First measurement is synchronous after saved preferences load, since a hidden
  webview cannot rely on animation frames. Subsequent layout reports coalesce, unchanged
  heights are skipped, and metric values are not resize triggers. Native caller/bounds
  checks reject unrelated windows and invalid numbers. Native placement preserves the
  tray anchor and clamps to current screen bounds and the existing 680 pt cap.
- A first open waits for measured content; another click cancels a pending first open.
  Hidden reports update cached opening size. WebContent termination invalidates it.
- Debug bench builds now retain their configured isolated identifier; normal debug
  builds still select the existing development identifier.

## Behavior checks and regressions

Applied ce:writing-tests with repository rules taking precedence. React integration
checks use real components/stores and the mock transport boundary. Browser fit/focus
checks use rendered bounds and accessible roles.

- Full frontend suite: 198 files, 1,288 tests passed.
- Focused defaults/pause/popover/brush integration run: 6 files, 59 tests passed.
  Popover behavior includes independent collapse, saved remount, hidden-card all writes,
  failed save preserving presentation, Retry recovery, no expanded flash while saved
  settings load, and visible sensor failure/missing values in compact presentation.
- Rust shell: 208 passed, 3 ignored; bench-feature shell: 207 passed, 3 ignored.
  Includes native unit probes for US/UK and explicit opposite-temperature overrides,
  injected default versus saved opposing units, density partial patch persistence and
  unchanged engine settings, first-open/cancel/reload state, top-edge/screen constraints,
  invalid height/caller rejection and bench identifier selection.
- Schema: 80 unit tests, 12 compatibility tests and 1 export test passed.
- Workspace clippy with all targets passed; TypeScript, Biome and architecture checks
  passed during implementation. Normal commit hooks must pass for the final commit.
- Affected browser batch: 81 of 82 cases passed initially across Chromium and WebKit.
  The remaining WebKit case exposed pointer-click focus loss. Explicit focus preservation
  fixed it, including Docker's presentation swap. Final compact/Memory browser batch:
  all 18 passed across Chromium and WebKit. Other affected checks included CPU/GPU/Memory/
  Disk brushing, open-pause explanation, batteryless omission, Timeline long ranges,
  popover navigation/scrolling, neighboring screens and both themes.
- Revert proofs: removing the five-second caption threshold fails its boundary/clipping
  regression; restoring capability-only battery visibility fails the Power toggle
  regression; replacing individual collapse with expanded fails persistence/navigation;
  removing injected temperature defaults fails its regression; breaking pending-open
  cancellation fails the native lifecycle regression. All mutations were restored.
- Full workspace tests, `make check`, bindings freshness and performance gates are deferred
  to the orchestrator's quiet integration run. No performance result is claimed here.

## Measured fit and visual comparison

At 360 × 680 CSS px, six healthy compact cards including optional Containers consume
386 px of intrinsic card content: six 56 px cards, five 8 px gaps and 10 px bottom inset.
Header is 58 px and footer 53 px, totaling **497 px**. Both themes and both browsers agree.
Combined Battery, standalone Battery when Power is disabled, and batteryless variants
all fit without vertical scrolling. At 360 × 300, cards scroll while the footer remains
visible. Mixed CPU/Containers expand/collapse retains focus in Chromium and WebKit.

Real AppKit panel on the M3 Max, unique bench identity:

| Presentation | Native frame in screen points |
| --- | --- |
| Expanded/mixed | x=1084, y=37, width=360, height=680 |
| Saved all compact, first open | x=1084, y=37, width=360, height=433 |
| Saved all compact, subsequent launch | x=1084, y=37, width=360, height=433 |

The native host exposes five hardware cards and no enabled Docker card. Its 433 pt
height matches the five-card browser arithmetic. These are CGWindow bounds, separate
from screenshot pixel dimensions. The same top edge is preserved. Native snapshots
show real metric values and correct first-paint compact preferences.

Screenshots are under ignored `test-results/screens/`: compact light/dark in Chromium
and WebKit, hardware pages, gallery/module-card comparisons, `native-expanded.png`
(mixed state) and `native-compact.png`. Compared against design-system.md, expanded
neighboring cards and the dev gallery. Intentional differences: compact headline values
are 12 px to fit directional network rates, compact cards target 56 pt, the persistent
chevron occupies a small heading slot, the Containers inset aligns with other cards,
and pause captions lose button-like borders. Expanded module typography/surfaces remain.

Native limitations: OS denied Accessibility automation. Scripted first-click timing,
tray hide/reopen, pointer-driven native collapse, WebContent kill/reload, display changes,
short physical screens and larger system text remain manual acceptance. Portable native
state/placement tests and browser focus/overflow checks cover their respective logic;
they are not claimed as end-to-end native acceptance.

## Native test setup incident

The first debug bench launch used the normal development identifier because existing
lib.rs unconditionally replaced the bundle identifier in debug mode. It wrote the
existing development history WAL; settings.json retained its prior date. The production
app process was left running and production data was not targeted. The new process was
stopped, the incident was reported, and development data was preserved without attempted
rollback or cleanup. The bench-specific identity guard and regression now prevent this.

Subsequent logs explicitly identify the fresh
`com.tryopendata.kelvo.bench.ux-native` data/log directories. One synthetic completed
onboarding fixture registered that isolated app's own login item; its setting was
immediately disabled and a subsequent native log confirmed `Not Registered`. Test-owned
native processes were stopped. No user login-item setting was changed.


## Durable source-query follow-up (D-120)

The remote protocol now negotiates `query.usage-history.v1` independently of
`query.v1`. Historical app queries and bounded process pages read the sampled
host's HistoryStore; controller mirrors are never an attribution fallback.
Offline hosts return remote_unreachable, old peers remote_unsupported, missing
source stores history_unavailable, stale/invalid continuations invalid_argument,
and busy reads remote_busy. App sorting uses an app-only recent snapshot;
process expansion snapshots only the requested app.

Native commands `query_historical_usage` and `query_usage_processes`, generated
bindings, native transport and deterministic mock transport share the Rust DTOs.
The mock includes bounded 125-instance paging, fixed comparison membership,
missing/pruned/gap/resource coverage, unavailable stores and remote refusal.
Rust exports the request/page bounds and default 750 MB cap. Onboarding displays
the configured cap, including preserved saved 150 MB choices.

Verification: TypeScript passes; all 199 frontend files / 1301 tests pass, including
35 focused cases across durable mock, Settings and onboarding. The nine durable
mock scenarios cover interval queries, membership,100 + 25 paging, invalid/stale
requests and restart, missing evidence, pruning/capabilities, remote/offline,
unavailable store and old peers. Four cap UI cases prove 750 MB / default options
and saved 150 MB preservation. Full remote tests pass 83 unit+7 socket sessions;
11 Docker/real-host/packaged environment cases remain ignored. Three new proto
round-trip/skew/negotiation tests pass; two existing shell serialization tests
were strengthened for source/cursor errors. Architecture checks pass.

Mutation proof: removing the advertised durable feature fails the negotiation
test; disabling the session feature guard fails the old-peer behavioral test.
Both were restored and rerun green. No budgets were increased.

Browser comparison: onboarding at its configured 820 × 566 viewport shows 750 MB,
Done remains visible and horizontal overflow is 0. Screenshots are in ignored
`test-results/screens/onboarding-cap750-dark.png` and
`history-contract-gallery.png`; the existing tokens/layout match the gallery.
The added cap phrase wraps one extra line intentionally. This is browser fit,
not new native geometry evidence. The screenshot's old series-only projection
estimate is awaiting the storage owner's measured-growth integration and is not
accepted as the final estimate. Only favicon 404 appeared in the browser console.

Production recording byte gates and final integration/performance checks remain
orchestrator-owned landing prerequisites. No push or full performance gate ran.

## Module recorded-attribution callers (D-120)

CPU, GPU, Memory, Power and Network now mount the shared read-only recorded
usage table for fixed intervals or rolling windows longer than the one-hour
recent ring. Short live ranges retain their existing search, process expansion
and process actions. Changing back to Live remounts those controls. Historical
Network requests the selected source host even when current Network recording
is off; current rates and capability flags never substitute for stored evidence.
Disk preserves recorded charts and shows ‘Per-app disk history isn’t recorded’
instead of mounting a recent process table. Fixed tables are memoized against
parent sampling renders; range, host and brush changes still update them.

Twelve new real-route behavior cases cover five resource pages, a rolling day,
short live behavior, Disk charts and attribution omission, capable remote source,
recording-off history, return to Live, and remote failure plus Retry of the same
interval. No internal components or hooks are mocked; only the failed transport
command is overridden in the Retry case. All twelve pass. Disabling the shared
card routing makes the CPU case fail on the missing visible Compiler row; after
restoration all twelve pass again. The initial implementation-free CPU case also
failed on that row. Sixty-four neighboring behavior cases pass. All TypeScript
projects and the full frontend suite pass: 206 files / 1332 tests. These results
used the shared table owner’s offered source handoff unchanged; dependency
da4fe55 was then integrated as 7d4e057 through normal hooks, and the combined
module/shared-card suites passed all 21 cases against that exact dependency.

Inspected browser screenshots of all five resource tables, Disk and the dev
gallery are under ignored test-results/screens/module-*. Existing SectionCard,
module accent, table and typography tokens match neighboring screens. The
intentional difference is the same five comparable recorded-resource columns
on each historical module, with the selected module ranking highlighted and
no process selection or Quit controls. Date, coverage, gaps and CPU-only power
estimate limits remain visible. At 1280 × 900 the document is 1280 px and main
is 1060 px, with zero horizontal overflow. At 820 × 700 the document is 820 px
and main is 600 px, also without overflow; the table scrolls internally to the
remaining columns and its footer. The module-memory-day-card-820-dark screenshot
records this intentional narrow-layout behavior. Only the existing favicon 404
appeared in the browser console. This follow-up changes no native geometry.

## Final integration regression follow-up

Release identity selection initially used cfg!, which still typechecks the debug
branch and referenced a debug-only constant in release. Commit d610ac5 replaces
that with compile-time cfg branches, including the regression expectation. The
debug identity test passes in the app crate; the exact extracted function and test
also compile and pass with debug assertions disabled. Normal Rust hooks pass.
The orchestrator owns rebuilding the full release candidate. No app was launched.

Performance-log review found no new heap work in process observation: it writes
fixed arrays of optional enum evidence. The existing first-seen identity path
allocates a path, optional argv buffer and String, app identity, Arc identities,
and sometimes path-component storage. The emitted-new-row counter only counts
processes that survive long enough to produce rates; short-lived processes may
allocate identities without ever appearing in this count. Commit 98f5157 corrects
the misleading one-name-allocation wording. SelfCpu and libproc implementation
are unchanged from main. Thread-local accounting excludes other threads directly;
external builds can change the measured workload through process churn. The
aggregate log cannot assign the residual tray engine-core excess to one source;
quiet baseline/candidate measurement remains required. No ceiling changed.

Browser integration assertions now target the module header range controls and
the range-status summary rather than matching every repeated date/control on a
page. Power explicitly checks that its Battery card follows both 30m and 5m.
The history Settings scenario now proves 90 days discloses shortening under
the measured 750 MB cap. All 18 cases in chart-window.spec.ts and
interval-history.spec.ts pass across Chromium and WebKit, including both themes
and existing axe checks. They ran against an explicitly started integration-tree
Vite server on port 1421, because this lane predates the measured-growth source;
no stale server was adopted. The server was stopped immediately after the run.
The Settings screenshot visibly shows 750 MB and limiting to about 45 days.

## Integrated acceptance, 2026-10-26

Final reviewed source is `aeff3d5f5e8db9bfc57357e4f97e619b2ff90b1a` on
[PR #3](https://github.com/tryopendata/kelvo/pull/3). The normal push passed the
full check, binding regeneration comparison and 13 production browser-performance
cases. Power readiness now scopes its header selector and separately asserts the
Battery card follows it; measurement duration and limits are unchanged. Power
measured 12.75 ms/s, popover 11.79 ms/s; neither produced a long task. The full
functional Chromium/WebKit run passed 476 cases without retries.

Both final release benchmark binaries are built and the candidate app signature
verified. The whole-app benchmark refused to launch because the lid was closed
with no external display. No whole-app CPU or footprint result is claimed.
An active display and measured tray frame counters are required before merge.
The completed engine pair and native/browser evidence are preserved under
`target/qa/product-ux/` in the integration and primary checkouts. The task-owned
`run-whole-app-comparison.py` resumes only the baseline/candidate app pair;
`run-cpu-comparison.py` must not be restarted over its completed engine databases.

Exact-source Linux CI passed all five jobs. Chromium passed 238 cases; WebKit
passed 235 on the first attempt and three on retry. Read-only Sol 6.1 triage
found no demonstrated change-related product defect: the Monitoring screenshot
timed out after its dialog assertions, Overview's unchanged settling test reads
running animations and opacity in separate calls, and the unchanged remote-host
dialog test stalled waiting for click actionability before invoking the connection
check. No runner-load cause or baseline CI flake is claimed without telemetry.
The local 476-case run had no retries. Source logs and job metadata are preserved
alongside the local evidence; this records their actual result, not a retry-free
hosted run.
