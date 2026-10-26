# Timeline implementation

Branch: codex/ux-timeline. Worktree: ux-timeline/kelvo.

Independent slice complete: shared persisted arbitrary ranges, session Back/Live,
keyboard/pointer selection, compact grouped annotations, selected-range authoritative
lane summaries, Battery charge history and local Containers shared bounds. Selection
projects the existing plot through a clipped one-shot transition, then crossfades the
finer read. Superseding ranges cancel the projection; reduced motion and performance
mode settle immediately. Failed settings writes keep the committed range and offer Retry.

Existing querySeriesStats supplies average, peak and integral with separate measured
coverage. Returned widened summary bounds are disclosed. No frontend authoritative
aggregation was introduced. Durable application attribution and comparison remain the
next slice after the storage contract and recording integration.

Verification: typecheck and formatting passed; 203 frontend files / 1299 tests passed.
74 browser cases passed across Chromium and WebKit, including both themes, drag release,
keyboard/cancel, cross-tab range propagation, Back/Live, Battery/Containers bounds,
annotation investigation, pause hover/focus explanation, reduced motion and performance
mode. Regression mutations reproduced failures for navigation, save recovery, selection,
event priorities/grouping/bounds, summary stale replies/errors, Battery bounds, superseded
zoom and browser drag commit. Retained-link precedence is tested before and after the
retained browser mounts. These use real internal modules and transport/time boundaries.

Screenshots were compared with the CPU neighbor and dev gallery:
/tmp/kelvo-timeline-before.png, /tmp/kelvo-timeline-zoom-motion.png,
/tmp/kelvo-timeline-after.png, /tmp/kelvo-timeline-7d-dark-final.png,
/tmp/kelvo-timeline-cpu-neighbor-dark.png, /tmp/kelvo-timeline-gallery-dark.png.
Intentional differences: lanes are 72px high so recorded value, summary and coverage fit;
the event track uses one compact labeled control per group; range controls match the
shared dashboard control; Battery displays recorded charge instead of hourly boolean
charging buckets. Existing type, surfaces and module accents remain.

Imported native-owner pause metadata/caption patches in history-state, gap-band,
use-gap-bands tests and lane-overlays overlap native commit 343cd8b. Parent will integrate
the identical changes. Parent owns plan/PROGRESS.md, integration and final review.
Full native/performance gates are deferred to the scheduled quiet integration run;
no performance acceptance claim is made here.

Next ownership: collector source completeness, engine usage recorder/recent ring and
HistoryStore query wrappers. Storage owns kelvo-store; native owns remote/shell/transport.

Recorder handoff: internal collector observations preserve unreadable, unprimed,
unresolved, name-limit and failed-read evidence without extra cadence or per-sample
allocation. All observed processes enter durable 10s facts before display floors.
Concurrent app RAM and unioned durations remain separate from process sums; GPU uses
its own read span. Exact network byte splits conserve integers; late bytes retain their
session-time uncertainty separately. Source locks cover nonblocking writer enqueue,
never SQL; query callbacks run after the durable snapshot. App queries copy no process
facts; expansion copies only the requested app's instances.

The source owns the open recorder and recent closed facts. This avoids per-tick history
copies. Clear generations are checked before collection and after the read; clear-history
invalidates both sides of its queued delete, including ticks during the operation. Pause,
stall, restart and clock holds establish fresh observation edges. Clear/clock-backstep,
queued replacement, failed commit/retry and collector-read reset races have real-engine
and SQLite behavior coverage.

Backend verification: 308 engine tests passed (5 hardware-only ignored), 187 collector
tests passed (23 hardware-only ignored), clippy and Rust size checks passed. Four mutations
reproduced assertion failures for coverage union, clear generation, disabled resources and
late-byte exclusion. All five unchanged allocation/call gates passed: windowGPU10.57,
network10.92, tray9.81, history-off9.15, window10.81 core allocations/tick. Log:
/tmp/kelvo-usage-allocation-modes.log. The engine write-volume gate still needs all seven
usage tables included; quiet release CPU and full integration gates remain parent-owned.
Storage source contracts integrated through 8deb33a. No user history files were modified.
