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
