# Product UX polish: defaults, Timeline investigation and compact popover

Status: implementation in progress; authorized on 2026-10-25.
Product choices and Devil's Advocate review corrections incorporated.

## Intent and agreed scope

Make Kelvo useful in three moments: a sensible first launch, a small menu-bar glance,
and a historical investigation that identifies which apps contributed to load.
Keep existing SF typography, module accents, card surfaces and transport boundaries.
Use the existing design system, not a new visual theme.

Sources: [issue #1](https://github.com/tryopendata/kelvo/issues/1), the user's Timeline
and popover screenshots, and the inspected current implementation. The issue describes
0.1.0; several batteryless behaviors already exist on main.

Confirmed decisions:

- Initialize temperature from macOS; preserve valid saved Celsius/Fahrenheit choices.
- Hide only prominent captions for closed pauses shorter than five seconds; keep gaps.
- Interpret the issue's cache graph as Memory's Swap chart and make it full width.
- The Battery module toggle also controls its section in Power & Sensors.
- Timeline selections zoom and become the common range for history-capable views.
- Include durable app attribution; do not fabricate old unrecorded data.
- Show ten app groups, expandable into process instances, with sortable resource columns.
- Remove raw event logs; thin annotations and make them investigable.
- Include Back/Live, historical lane summaries, lane-to-table sorting, coverage,
  a secondary activity calendar, and comparison with the preceding interval.
- Animate selection changes gracefully with existing motion and accessibility rules.
- Let users independently collapse popover cards while retaining navigation and a
  useful small visualization. Aim for no scrolling when all cards are compact.
- Shrink the popover to fit its content, capped by available screen height.

Defaults chosen for the implementation: new popover preferences start expanded;
network ranks by total transferred bytes; RAM ranks by peak concurrent app footprint;
the app table initially ranks by average CPU. Preserve the existing global, persisted
chart-range scope across hosts/windows rather than add a second range owner. The user
confirmed content-fitting popover height, capped by available screen height; retain
the existing 680 pt maximum as the implementation default.

## 1. Issue #1 changes

### Temperature

Resolve the local Mac's unit in the app shell when constructing first-run or invalid-
settings fallback defaults. Inject that default into the settings loader for tests.
Use Foundation measurement formatting through the existing Objective-C integration:
compare automatic locale formatting with explicit Celsius/Fahrenheit formatting of
the same value, accounting for the temperature override independently of region.
A read-only probe verified US, UK and explicit temperature override locales.
If resolution fails, keep the existing default. Do not change valid saved settings.
No new System option, preference watcher, settings enum or protocol change is needed.

### Battery and Memory

Reuse capability/module-state logic. Verify existing batteryless omissions in sidebar,
popover, Overview and Power. Align direct Battery-route handling for missing and
not-present capability after capability loading; do not equate unavailable with absent.
Use the effective Battery state for the battery section in Power & Sensors. Remote
hosts continue to ignore local module toggles. Keep the disabled Not present Settings
row and retain saved module preferences; keep useful desktop system-draw information.

Stack Memory's Pressure/Used and Swap charts at full width. Remove obsolete grid spans
so narrow layouts do not create implicit columns. Apply consistently to Mac and Linux.

### Historical pauses

Keep engine pause timing, recorded gaps, masks and line breaks unchanged. Carry reason,
original closed duration and caption visibility in frontend presentation types.
Evaluate the five-second threshold before visible-range clipping. Closed pauses under
5,000 ms have no prominent caption; exactly 5,000 ms qualifies. Open pauses keep their
hatching and accessible explanation; current status explains the ongoing pause.

Use a muted pause glyph and plain Recording paused here text, with hover/focus text:
Recording was paused during this interval. No samples were collected.
Remove control-like styling for these captions in both shared bands and Timeline.
Keep widgets render-only and retain zoom-dependent caption suppression. Caption hit
targets must not block the chart's brushing or keyboard cursor interaction.

## 2. Timeline experience

### One range and reversible navigation

Timeline adopts the existing ChartWindow relative/absolute contract and Rust-owned
general.chart_window setting. Replace fixed-Span assumptions throughout history
queries, cache keys, axes, event reads, heatmap navigation and CSV export with duration
and bounds. Presets remain shortcuts. Use returned bucket resolution and explicit gaps.

Drag anywhere across metric lanes to preview a range without queries. Pointer release
commits one absolute range and reloads all lanes. A click still inspects the crosshair.
Use pointer capture; Escape/cancellation restores the previous view. Snap outward to
displayed bucket boundaries with a minimum of one minute or one displayed bucket,
whichever is longer. Existing date controls provide a non-drag path; preserve keyboard
inspection and add keyboard range selection. Disable selection until plot bounds exist.

Maintain a bounded dashboard-session Back stack (20 committed views). Back restores
the prior range, including whether it followed Live. Existing previous/next controls
move by one displayed duration and remain distinct from Back. Live resumes the last
relative duration used in this dashboard session; after a reload with an absolute
selection, use 24 hours. Saving failures leave the old committed range and allow retry.
Do not push changes from another window or duplicate settings events onto the stack.

Range consumers:

- Timeline and CPU/GPU/Memory/Power/Network/Disk history and attribution share bounds.
- Battery's history section becomes range-aware using recorded charge history;
  retain explicitly labeled current health/charge/status controls. Historical charge
  must not infer charging intervals from an averaged boolean.
- Local Containers adopts shared bounds while retaining its other URL filters. An
  explicit incoming range link applies its range once; later range changes update it.
- Overview, Processes, popover, current sensor/health readouts, Disk Space and cleanup
  stay current. Remote Linux Containers stays current until historical inventory exists.
- History views visibly name fixed dates and timezone; current sections say Current.
  Exports use the same selected bounds. Other hosts show data for those same UTC bounds.

### Zoom animation

Use a one-shot transition within existing motion tokens. Project the already-loaded
plot into the selected viewport under clipping; crossfade to finer returned data when
ready. Do not morph sample values, bridge gaps, or present coarse preview values as
newly fetched detail. Keep the old resolution label until the new response is displayed.
Show loading feedback during longer reads, retain context on errors, and ignore stale
responses after another selection or host switch. Disable hit testing while geometry
is transitioning, then restore focus. Reduced motion and Performance mode settle
immediately. No per-tick zoom animation, increased sampling or hidden-window work.

### Attribution and useful historical context

Place Apps in this range directly below metric lanes, before secondary workload charts
and the activity calendar. Reuse shared app-table pieces, identity formatting,
expansion, coverage and sort controls; do not duplicate an unrelated table framework.

Columns: App, Avg CPU, Avg GPU, Peak RAM, CPU power est., Network. Display CPU and GPU
with established project units/normalization; RAM in the selected memory units;
CPU energy divided by measured duration in watts; network total RX+TX bytes with
direction breakdown in detail. State that CPU power is estimated CPU energy, not
whole-device consumption. Unavailable metrics stay unavailable, including Linux energy.

Selecting a resource column ranks all recorded app identities for the interval in Rust,
then returns ten. Reversing the sort order reverses those ten; it does not change to
the ten lowest users. Clicking CPU/GPU/Memory/Power/Network lane labels selects the
matching ranking. Temperature has no defensible process-temperature attribution and
does not set a ranking. Process expansion is read-only, lazy and identity-safe; no
historical row inherits current Quit/Force Quit actions. Network children say app-level
only when per-process evidence is unavailable. Process RAM peaks need not add to app peak.

Lane summaries describe the displayed recorded interval, not current readings: CPU/GPU
average and peak, RAM peak used, average system power, maximum temperature, and network
bytes. Use authoritative returned statistics for aggregations shared across consumers.
Crosshair values remain momentary and visibly distinct. Show concise per-resource
coverage/resolution, including partial retention, pauses, failed reads and unmeasured GPU.

### Events and hierarchy

Remove raw Docker event-link lists and per-gap diagnostic prose from Timeline. Retain
useful workload charts, errors and a single human-readable coverage summary. The
Containers page retains detailed events. The activity calendar stays below attribution.

Preserve each event's identity, start_ms, detection timestamp and cluster members.
Do not treat detection time as proof the episode ended. Group near-overlapping markers
by available horizontal pixels, preserving member lists and original counts. Prefer
alerts and worsening serious/critical thermal transitions, then sustained load and
power spikes, then repeated fan events. Recovery is not styled as a warning. Do not
change detector thresholds or recording to reduce visual noise.

Use a compact annotation track with focus/hover details. Only the hovered, focused or
selected event/cluster draws a full-height guide or interval highlight. Retain discrete
markers for lower-priority events within groups; no unexplained +N that discards members.
Zooming reveals more markers as space becomes available. Keep keyboard access to groups.

Click an episode to select its start through detection plus 20% padding on each side,
at least one minute per side, then apply range/bucket constraints. Instantaneous events
open five minutes on either side. Clamp to known retained bounds and host now without
creating an invalid interval. Click a cluster to select its members' union with the same
padding; if it cannot zoom further, expose its member list for individual inspection.

### Compare with the preceding interval

Offer a Compare previous interval toggle for a fixed selection. Freeze the active
range; compare against the immediately preceding equal-duration interval. Keep aligned
metric lanes primary and show comparison/delta values in lane summaries and app cells,
without doubling all chart lines. Membership is the current interval's top ten by the
selected metric; fetch previous values for those identities, not an independently
truncated top-ten list. Missing previous coverage shows unavailable, never zero. An
absent app can be zero only with complete observation. Do not change the global range
when enabling comparison. Live clears comparison. Compare remains off by default.

## 3. Durable app and process history

Existing query_usage_by_app covers about one hour in memory and starts empty after a
restart. Persisted snapshots are CPU-selected top thirty, later top five; they lack
the identity, GPU and energy facts necessary for this feature. Stored network buckets
also fold smaller names into Other. None of these are valid sources for exact long-range
multi-resource rankings. Old periods must say attribution was not recorded.

Record mergeable app and process-instance aggregates before ranking, floors or top-N
truncation. Use the existing app-identity rule; process keys include PID and start time.
Preserve CPU/GPU time integrals, CPU energy joules, network directional byte totals,
app concurrent RAM maxima and RAM byte-time/present-time. Store measured duration and
coverage per resource separately from host coverage. App RAM peak is the maximum of
the simultaneous sum, not the sum of each process's peak. Network remains app-level
where no process-instance evidence exists; unresolved bytes remain unattributed.

ProcessNet late_rx_bytes/late_tx_bytes have uncertain transfer time. Do not copy the
existing accumulator's oldest-open-bucket assignment into interval attribution.
Persist them separately with the known possible-time interval (or collection-session
bounds when finer timing is unavailable), and mark overlapping network queries
incomplete. Exclude them from exact interval app totals; expose the uncertainty without
claiming they were transferred at identity-resolution time. Verify an owner discovered
after the selected interval ends. Host transfer totals remain distinct from attributed
app bytes and do not lose the underlying observed traffic.

Use existing 10-second, one-minute and fifteen-minute age tiers and configured retention.
Roll down by sums/maxima with all identities preserved, then aggregate and rank for a
query. Retain process detail for the same horizon, loaded only on expansion. Intern
identities, batch writes and prune whole old intervals through the existing size-cap
mechanism. The approved default cap is 750 MB; retain valid saved cap choices and
disclose actual available dates when a smaller cap fills.
Do not silently cap identities or evict minor apps from each bucket to meet a budget;
any unavoidable resource-limit loss makes coverage and rankings explicitly incomplete.

Return requested bounds, actual covered bounds/duration, resolution, capabilities and
per-resource completeness along with rows. For boundaries inside a retained bucket,
use finer data where retained; otherwise exclude partial boundary buckets and disclose
the uncovered edges. Do not prorate a peak or integral into invented sub-bucket precision.
Permit incomplete observed rankings, clearly labeled; never promise exact rankings
for data the source cannot observe. Reconcile disk and recent uncommitted data without
double counting. Clear-history, resets, pruning, sleep, clock changes and module changes
must apply to the new data and its coverage too.

Completeness is a collector/source fact, not inferred from elapsed covered duration.
Preserve explicit causes such as unreadable processes, a first sample without a rate
baseline, unresolved identities and the existing network new-name cap (128/hour).
Propagate those causes through stored rollups, query replies, remote replies and
comparison. Existing collector limits and permission boundaries stay intact. A host
that sampled every scheduled interval can still have incomplete app attribution;
missing app rows in that case are unknown, not a proven zero.

Page process detail with bounded, deterministic, snapshot-bound continuation; keep
full retained instances without returning an unbounded list. Return a bounded network
uncertainty summary while retaining raw uncertainty facts. App rankings read app
aggregates independently of process-detail blocks. Validate request bounds, comparison
membership (at most the current top ten) and page inputs.

Add typed durable query and lazy process-detail interfaces through schema, transport,
engine/store and mocks. Write the data-model/protocol decision before implementation.
Do not silently change the current live-ring query contract. Negotiate remote support
using existing request/reply machinery: capable connected hosts answer from source
history; older peers report unsupported; offline peers report unavailable. Offline
mirroring of the new usage data is outside this batch and must be explicit in UI.

Collector cadence and permission boundaries stay unchanged. New storage must pass
existing idle/performance and storage gates with real measurements, including high
process churn. The user approved storage-write ceilings of 4 MB payload/hour, 12 MB WAL/hour
and 10,000 rows/hour during execution after the original limits failed feasibility
probes. CPU budgets, collection cadence and permissions remain unchanged. If full
retention cannot fit, measure a larger disk cap and ask before changing it or cutting
process detail. The user chose this measurement over accepting shorter retention.

The existing store-volume gate enumerates tables: extend its row/payload accounting to
all new usage, identity and coverage tables, rather than let new tables evade it.
Measure payload/WAL growth, restart recovery and long-range query latency under steady
and high-churn workloads before dependent Timeline implementation. The original ceilings of 1.2 MB payload/hour and 6 MB WAL/hour failed the
feasibility probe. Approved execution ceilings are 4 MB payload/hour, 12 MB WAL/hour
and 10,000 rows/hour; the source budget file remains authoritative.
This feasibility gate is blocking. If full-horizon process detail cannot satisfy the
unchanged budgets, return a concrete measured retention/cardinality tradeoff for the
user; do not label the whole batch implementable or silently drop identities.

## 4. Compact popover

### Interaction and saved preferences

Give every current display card Expanded and Compact presentation, independently
saved. An always-discoverable chevron is a separate button beside the card heading;
it has an accessible Collapse CPU / Expand CPU name and aria-expanded. The rest of
the card keeps its existing dashboard navigation. Do not nest a button in the card
link; reuse the existing sibling-control pattern. The toggle never opens the dashboard.

The header's settings control opens a small Popover options menu containing Collapse
all, Expand all and Open settings. Keep the header's width and existing sampling/pause
controls. Collapse/Expand all writes the complete density choice for all known card
IDs in one settings patch, including currently hidden cards. Individual changes allow
mixed density. Apply saved state on first content paint to avoid expanded flash.

Rust owns a new popover settings group with density per display-card identity:
CPU, Memory, GPU, Power, Network, standalone Battery and Containers. Missing fields
default to Expanded. Use generated bindings and partial patches through transport;
no localStorage or widget-owned stores. Save errors retain/recover the previous state
and provide one concise retryable notice. A density update does not alter module
enablement, collector demand or menu-bar preferences.

Power & battery is one existing display card and uses Power's density. Standalone
Battery retains its own preference while hidden. Do not duplicate Battery when Power
already includes it. Disabled/absent modules remain omitted. Unavailable placeholders
use the associated card density but preserve their explanation. Containers participates
when its existing show-in-popover preference is enabled. Do not add a Disk card here.

### Compact content and size

Use existing surface/accent rules with a target 56 pt healthy card height and 8 pt gaps.
Keep title, headline value, units and one small visualization. The graphic is secondary
to the number; no dense legends, chart axes, addresses, stat grids or descriptive prose.
Keep a small shared or accessible statement of the sparkline window; compact history
uses the same scaled window and gaps as expanded history.

| Card | Compact information |
| --- | --- |
| CPU | Total percent and a short utilization sparkline; omit user/system bars |
| Memory | Used/total and a small usage meter; retain pressure state text, especially warning/critical |
| GPU | Utilization and sparkline; omit frequency/power/render detail |
| Power & battery | System watts and power trend; battery percentage plus charging/on-power indication when applicable |
| Network | Upload/download rates with direction and units, plus small mirrored history; omit addresses |
| Battery alone | Charge, charging/on-power state and a thin charge meter; omit health/cycles/time estimates |
| Containers | Running count and CPU/working-set figures with a small running-count meter; retain disconnected/partial status |

Warnings and read failures cannot disappear on collapse. Use concise visible status
and a focus/hover explanation; permit height growth for essential notices. No zero
substitution for missing values. Use stable numeric slots and accessible chart labels.

Fit acceptance: at the normal 360 x 680 pt viewport, every supported healthy card in
Compact mode, including optional Containers and standalone/combined Battery variants,
fits without vertical scrolling. Verify actual rendered heights rather than relying
on the estimate. Fit the native panel to header + intrinsic card content + footer,
capped at the existing 680 pt maximum and the current screen's available height.
Keep the panel anchored under the tray icon at its top edge and clamped to screen edges.
Short screens, larger text and multiline errors retain scrolling rather than clipping.

Measure intrinsic content separately from the viewport: the existing h-svh/flex-1
layout must not feed the resized viewport height back as its content request. Add a
typed, bounded transport command owned by the popover shell; ignore invalid requests
and requests from unrelated windows. Coalesce layout-triggered updates, skip unchanged
sizes, and re-evaluate native placement for the current tray anchor/screen. Do not
poll, use metric ticks as resize triggers, or add a second height animation that
fights native positioning. Changes while hidden apply on next open; saved density
must be known before choosing the opening size. Retain native hide/occlusion behavior.

Render only the selected presentation body, reusing existing chart/meter primitives;
do not leave hidden expanded charts running. Preserve per-card narrow subscriptions,
missing-data semantics and motion settings. Collapse is a user-triggered layout change,
not a per-tick animation. Avoid adding collection or periodic history requests for
compact-only ornament; use already-available live series and container state.

## 5. Delivery and acceptance

Plan review precedes implementation. The orchestrator dispatched ce:devils-advocate
as a Sol 6.1 agent against this document and repository evidence; corrections and
the outcome are recorded below.

Execution uses Sol 6.1 agents with isolated worktrees. Keep shared-file ownership clear:

1. Native/settings lane: temperature defaults and compact preference contract, generated
   bindings/mocks coordinated with the UI owner. Issue #1 UI and compact cards follow.
2. Storage lane: durable facts, rollups, coverage, local queries, remote negotiation and
   budget evidence. Publish typed contracts before dependent interface work starts.
3. Timeline lane: shared range, animation, annotation grouping, attribution integration
   and previous-interval comparison. It depends on the storage query contract.
4. Integrated review, visual verification and landing. Avoid competing workloads during
   performance checks. Agents commit/push their branches; the orchestrator owns merging
   and managed-worktree cleanup. No release or external issue reply is included.

Verification includes:

- Temperature default injection, region overrides and saved opposing units.
- Battery capability/loading/disabled/remote combinations and full-width Memory layouts.
- Pause boundary durations, clipping, open-to-closed gaps, focus/hover and brushing.
- Timeline arbitrary bounds, one commit per drag, keyboard/cancel, Back/Live, save failures,
  tab/window/host changes, stale responses, heatmap, exports, retained-tier edges and DST.
- Real animation screenshots, reduced motion, Performance mode, loading/error states and
  no sample/gap invention. Cursor and focus remain usable after the transition.
- Events: dense fan clusters, serious thermal events, recoveries, retained members,
  instance/group clicks, edge clipping and no full-height line forest at 7d/30d.
- Durable data: restart recovery, old unavailable periods, correct per-column winners,
  ranking after interval aggregation, app concurrent RAM peaks, process reincarnation,
  gaps/partial boundaries, rollups, prune/reset and uncommitted/disk reconciliation.
- Comparison: current top-ten membership, previous values for the same identities,
  missing/partial prior coverage, zero only with evidence, no global range changes.
- Remote capable/old/offline hosts and per-resource unsupported states.
- Compact persistence, mixed/all states, no navigation from toggles, navigation from
  cards, combined/standalone Battery, optional Containers, disabled/re-enabled modules,
  missing readings, notices, 360 x 680 no-scroll and short-screen overflow fallback.
- Native content sizing: no measurement feedback loop or per-tick resize, fixed
  top-edge anchoring, display changes, screen clamping, hidden/reopen behavior and
  collapse after scrolling near the bottom. Retain focus on the initiating control.
- Browser screenshots in both themes against adjacent views and the dev gallery.
  Record intentional differences in the design-system/execution notes.
- Revert-verify regressions. Run scoped tests while iterating, full functional browsers
  and normal commit/push hooks for final code/bindings/performance checks. Use Linux-only
  hosted CI and local macOS native checks, verifying the exact pushed revision.

Update plan/PROGRESS.md with actual execution status and evidence as work lands.
No production performance, native fit or full app behavior is claimed by the planning
prototype; it contains illustrative data only.

## Review record

2026-10-25: ce:devils-advocate, run by a Sol 6.1 agent, reviewed product behavior and
technical architecture against source. Verdict: pass after three targeted corrections.

- P1, uncertain-time network bytes: separately preserve uncertainty, exclude them
  from exact interval totals and test delayed ownership across the selected boundary.
- P1, source completeness: preserve unreadable/unprimed/unresolved/capped observations
  across storage and replies; do not infer complete attribution from covered time.
- P2, storage feasibility: account for new tables in the gate and measure churn,
  retained size and query costs before dependent attribution UI work.

All three are incorporated above. The reviewer also identified native placement and
viewport-measurement constraints for content sizing; those are included and the user
subsequently confirmed the content-fitting behavior.
The same reviewer re-read the corrections and native sizing addition and returned
Pass with no remaining material product/architecture finding.
The review did not establish runtime/storage feasibility or verify implementation.

## Execution record

2026-10-25: execution authorized. Orchestrator owns integration and landing; Sol 6.1
agents own implementation in isolated managed worktrees.

- [x] Native/settings and compact popover, including issue #1 fixes (integrated; final review/gates pending).
- [ ] Durable usage contracts and blocking storage feasibility measurements.
- [ ] Durable usage lifecycle, local/remote queries and correctness checks.
- [ ] Timeline/shared range, event navigation, attribution and comparison (depends on history gate).
- [ ] Integrated browser/native verification, independent code review and fixes.
- [ ] Normal hooks, exact-revision Linux CI, landing and task-owned cleanup.

The storage-write increases and 750 MB default history cap recorded below are
authorized. CPU budgets and full retained process-detail requirements are unchanged;
existing valid saved caps remain respected.

Execution checkpoint: native settings/temperature contracts compile and generated
bindings are produced. Independent Timeline navigation is in progress. The history
lane measured an existing native store baseline of 936 rows/hour, 769,392 payload
bytes/hour and 4,331,712 WAL bytes/hour. Candidate packed SQLite usage fixtures exceed
the unchanged storage gates under process churn even with lossless batch compression.
These are feasibility probes, not completed production implementation; authoritative
measurements and any required user decision will be recorded before attribution UI.

2026-10-25 user decision: raise storage-write ceilings to 4,000,000 payload bytes/hour,
12,000,000 WAL bytes/hour and 10,000 rows/hour. The prototype suggested these could
cover 1,000 processes with moderate churn, but production must pass them. CPU budgets,
sampling cadence and permissions remain unchanged. The user requested a measured
larger disk cap needed for 30 days before deciding; neither a cap increase nor
silently shorter retention is approved. Storage lane owns the decision entry and
authoritative budget update.

Additional execution requirements confirmed by the user:

- Keep approximately 24 hours at the finest recorded resolution, then progressively
  coarsen older buckets. Preserve accurate integrals, averages, concurrent RAM peaks
  and coverage; show retained granularity and uncovered partial boundaries. Sampling
  cadence is unchanged. Evaluate this policy in the 30-day disk-cap measurement.
- Apply ce:writing-tests throughout. Prefer behavior-focused integration and complete
  workflow tests, real internal modules and SQLite, with mocks only at repository-
  approved transport/collector/time boundaries. Cover visible outcomes, persistence,
  failures and correct rollups. Prove regression tests fail without their fixes.

2026-10-25 user decision: approve a 750 MB default history disk cap after the
30-day rolled-up SQLite prototype measured 445,186,048 bytes for new usage history.
Existing valid saved caps stay unchanged; smaller caps may retain fewer days, which
must be disclosed. Production fill measurement must verify the estimate. The approved
retention policy keeps approximately one day at finest recorded resolution, then
progressively wider accurate aggregation buckets.

Native/UI checkpoint: source commit 343cd8b4ab8097f83affe6884b95466600e07b0a
integrated as 0bef7ff. Focused behavior/visual/native evidence is recorded in
[ux-native-execution.md](ux-native-execution.md). Normal commit hooks passed; final
integrated suites, review and landing remain pending. Remote/shell/transport work
now belongs to that agent; the Timeline agent will implement collector/engine
recording while the history agent owns store/rollup/query behavior.
