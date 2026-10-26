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
