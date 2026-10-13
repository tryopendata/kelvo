# RFC: Containers that explain the load

Status: COMPLETED. Date: 2026-10-13. Verification and measured performance limits: [PROGRESS.md](PROGRESS.md#2026-10-13-containers-monitoring-ux-execution).
Owner decisions in this document are final unless marked "open".

This document is written for an implementer with no prior context. Every behavior is a
numbered requirement (`R-…`). Every task names its files. If something you need is not
specified here, stop and ask the owner; do not invent behavior.

## 0. How to use this document

1. Read §1 (what is wrong) and §2 (what we are building) once.
2. Implement the slices in §9. Slices A and B can run in parallel. C needs A and B. D needs C.
3. Each slice lists its requirements, tasks and done criteria. A slice is done only when
   every done criterion is met and recorded in `plan/PROGRESS.md`.
4. Repository rules still apply: `CLAUDE.md`, `.claude/rules/*.md`, `plan/design-system.md`.
   This RFC overrides the design system only for the amendments in §8, and slice A updates
   `plan/design-system.md` to match.

Glossary:

| Term | Meaning |
|---|---|
| Engine | One local Docker daemon (OrbStack, Docker Desktop, Colima). Kelvo monitors one at a time. |
| Snapshot | The live `DockerSnapshot` pushed by `transport.dockerSubscribe` (about 1 Hz while visible). |
| Bucket | One history row: 10 s (`docker_10s`), or a 1 min / 15 min rollup (`docker_1m`, `docker_15m`). |
| Weight | `weight_ms` on a stored metric: how many milliseconds of the bucket were measured. |
| Coverage record | A `docker_coverage` row (`DockerCoverage` on the wire) saying part of history is missing, and why. |
| Charted set | The containers drawn as lines on the comparison chart. At most 6. |
| Monitored set | Running containers automatically admitted from the selected daemon inventory, at most 128 active collectors. Not the charted set. |
| Slot | One of six fixed series colors, `--color-series-1` to `--color-series-6`. |

## 1. Problem and evidence

The Containers tab (`/dashboard/containers`) was rated 2/10 by the project owner on
2026-10-13. The owner's job is: "which of my containers is using CPU or memory, and which
one caused that spike earlier?" The current page cannot answer it:

1. The chart is one aggregate line. `route.tsx` calls `dockerHistory` with
   `container_id: null`, and `ContainerHistory` (`_components/container-history.tsx`)
   draws one path.
2. On real data the chart is fragmented (on the mock it looks fine). Cause: §1.1.
3. A list of `Coverage gap: write_failed` paragraphs, one per record and with no height
   bound, fills the card (`container-history.tsx:167-174`). Cause: §1.1.
4. Each table row starts with a checkbox (`ContainerMonitor`, `container-table.tsx:27-56`)
   that silently changes which containers are collected (`route.tsx:142-151` saves
   `explicit_selection: true`). Users read it as row selection.
5. The table pages 32 rows with no max height (`container-table.tsx:292-294`), and
   "Runtime processes" and "Workload review" sections sit between the chart and the table.

All paths in this document are relative to the repository root. Route files live in
`src/app/routes/dashboard/containers/`.

### 1.1 Confirmed history bugs (measured on the owner's real database)

Method: a read-only `.backup` copy of
`~/Library/Application Support/com.tryopendata.kelvo/history.sqlite`, covering about 5 hours
of OrbStack history (07:45 to 12:52 local, 2026-10-13), queried with the scripts in
Appendix A.

The stored data is mostly complete. For each of the five running containers
(liveoak-api-1, liveoak-worker-1, liveoak-caddy-1, opendata-grafana-dev, opendata-tempo)
about 95% of 10 s buckets exist, and over 99% of those carry essentially full weight. The
page looks fragmented because of two bugs that hide or mislabel saved data.

**Bug 1: a strict coverage check drops about half of the points.**

- `point()` in `crates/kelvo-engine/src/docker/persistence.rs:1802-1838` sets
  `covered = measured > 0 && measured >= expected`. `measured` is the smaller of the CPU and
  memory weights; `expected` is the full bucket (10,000 ms).
- `observe()` (`persistence.rs:1399`) accumulates weight from each container's reported
  `interval_ms`, not from wall-clock contiguity, so consecutive observations leave
  millisecond holes. A typical bucket has `weight_ms = 9999`.
- Measured per container: 45–47% of buckets pass. 52–54% fail by 100 ms or less. Under 1%
  fail by more.
- `ContainerHistory` drops every point with `!p.covered` (`container-history.tsx:57`), so
  about half of the good measurements disappear.
- The check also couples CPU to memory: full CPU with short memory hides the CPU value.

**Bug 2: the Docker write queue overflows between commits and leaves false loss records.**

- `Writer::write_docker` (`crates/kelvo-store/src/writer/docker.rs:70-110`) admits a batch
  only while `pending_writes < MAX_PENDING_WRITES` (128, `crates/kelvo-store/src/docker/mod.rs:32`).
- A successfully applied batch frees its slot only when the transaction commits
  (`finish_docker(.., Persisted)`, `writer/txn.rs:119-121`). Commits happen every
  `DEFAULT_COMMIT_INTERVAL` = 300 s (`crates/kelvo-store/src/lib.rs:79`).
- So about 3 minutes after each commit the queue is full, and every batch is rejected for
  about 2 minutes until the next commit.
- Each rejection calls `Admission::lose`. The next commit then writes an engine-wide
  `write_failed` row to `docker_coverage` (`docker_write_losses`, `writer/docker.rs:449-492`).
  The engine also records an in-memory `write_failed` gap on the rejected ack (`drain`,
  `persistence.rs:1555-1570`).
- The owner's database has 58 such rows: one every ~5 minutes from 07:48 to 12:52, each
  ending within 1–2 s of a commit (commits ~299 s apart), 1.9 h in total.
- The rejected buckets were not lost. They stay in the engine's one-hour ring with
  `revision > durable` and are re-sent in the next batch (`persistence.rs:397`). Each
  container has about 550 saved buckets inside the "failed" spans. The coverage rows are
  never removed, so the UI reports failures that did not happen.
- Host (non-Docker) history has no `write_failed` gaps in the same window. Only the Docker
  path is affected.
- Nothing is logged. `~/Library/Logs/com.tryopendata.kelvo/kelvo.2026-10-13.log` has no
  WARN or ERROR lines.
- Secondary defect: the allowance check in `handle_docker` (`writer/docker.rs:150-163`) adds
  `pending_bytes` (which still includes batches already applied to the open transaction) to
  `docker_used()` (which already counts them). Applied-but-uncommitted bytes count twice.

**What a rejected batch's retry does and does not cover:**
- Buckets are retried (ring plus `durable` revision).
- Docker events are queued one batch per event with no retry (`persistence.rs:530-554`).
- Collector coverage rows are sent once, when first seen (`persistence.rs:364-378`).

**Not explained, and out of scope unless a slice says so:**
- The remaining ~5% of missing buckets. The app was not running for at least 244 s in the window.
- The aggregate (`OrbStack`) entity has 14% of its buckets with zero CPU and memory weight.
  Weight is zero when the population is incomplete (`persistence.rs:1438-1452`).

## 2. Product decisions

The Containers tab is a monitoring page that attributes load to containers. It is not an
inventory or settings page. It follows the conventions of `docker stats`, Docker Desktop's
container list and Stats tab, Grafana/cAdvisor per-container panels, and Datadog
container views.

| Job | What the page must make easy |
|---|---|
| My Mac is busy now | See the container total. See the top consumer in the first table row. Open it. No configuration. |
| Something spiked earlier | Pick a time range. See named per-container lines, including stopped and removed containers. Find the spike. Open that container's detail at that time. |
| I am watching one project | Scope the whole page to one Compose project and compare its containers. |
| Is it quiet or broken? | Tell apart "measured zero", "history missing", "events uncertain" and "not being saved" without reading internal reason codes. |
| Stop or start something | Stop a running container or start a stopped one from its row. |

Out of scope for this RFC:
- Restart, pause and remove actions; multi-row selection and batch actions.
- Alerts, and advice such as "unused", "safe to stop" or "leak".
- Popover, tray and widget changes; remote engines.
- Project aggregate series; showing CPU and memory charts at the same time.

## 3. Page layout

**R-1.** Activity view order, top to bottom, at every width:

```text
PageHeader: "Containers"   OrbStack · Connected            [Monitoring…]
SegmentedControl: Activity | Storage & cleanup
Summary line + project scope
Comparison chart card (chart, legend, at most one notice line)
Table card ("Containers now", search, state filter, View menu, rows)
<details> "Runtime & history" (collapsed by default)
```

Sketch (values are illustrative):

```text
CPU 240% (2.4 cores)   Memory 1.8 GB   5 running · 11 total     [All projects ▾]

┌ CPU by container            [CPU | Memory]   [15m ▾] [‹] [Live] ┐
│ % of one core                                                     │
│ 200 ───────────────────────────────────────────────────────────── │
│       up to six named lines on a shared, timestamp-positioned axis │
│   0 ───────────────────────────────────────────────────────────── │
│      −15m            −10m            −5m                     now  │
│ ━ api  ━ worker  ━ caddy  ━ grafana  ━ tempo   +6 more            │
│ [notice line, only when something is wrong]           Details (2) │
└───────────────────────────────────────────────────────────────────┘

Containers now        [Search containers…] [All states ▾] [View ▾]
   Name / project           State      CPU    Memory          Net
■  api · liveoak            Running    160%   680 MB       1.2 MB/s    [Stop]
■  worker · liveoak         Running     70%   498 / 512 MB 0.1 MB/s
   redis · liveoak          Exited      —      —            —          [Start]
   … bounded, scrolling body with sticky column headings

▸ Runtime & history
```

**R-2. Status-copy budget.**
- When everything is healthy, render only the summary line, the project scope and the
  chart card (title, controls, plot, legend) between the header and the table.
- That means no coverage text, ranking qualifier, row-order text or membership text.
- When something is wrong, the chart card shows exactly one notice line (R-41).
- Remove the "Order paused / Reorder now" bar (`container-table.tsx:401-414`). Held order
  (R-34) shows no text.

**R-3.** Remove these from the Activity view:

| Item | Location | Where it goes |
|---|---|---|
| Coverage status line | `route.tsx:349-368` | Removed |
| "Runtime processes on this Mac" details | `route.tsx:389-414` | R-5 |
| `DockerActivityReview` "Workload review" section | `route.tsx:415` | Delete `_components/docker-activity-review.tsx`; row actions replace its Stop button |
| Footer checkboxes and Disable button | `route.tsx:459-504` | R-40 |
| `RetainedHistory` after the table | `route.tsx:509-511` | R-5 |

**R-4. Summary line.** Live values only:
`CPU {cpu}% ({cores} cores)`, `Memory {working set}`, `{running} running · {total} total`.
- Source: `snapshot.aggregate` (`cpu_cores × 100`, `working_set_bytes`, `running`, `total`).
- With a project scope (R-10), prefix the project name. Show the sum of the in-scope
  containers' current `cpu_pct` and `working_set_bytes` from the snapshot, and that
  project's running and total counts.
- If any running in-scope container has a null value, append ` · partial`.
- Never show engine capacity (`capacity_bytes`) on this line.

**R-5. "Runtime & history".**
- One `<details>` element, collapsed by default. It contains, in order: the runtime
  processes block, the persistence line, and `RetainedHistory`, each moved unchanged.
- Rendered when monitoring is enabled and the Activity view is selected.
- When monitoring is disabled, keep today's behavior (intro, Enable button,
  `RetainedHistory enabled={false}`).

**R-6. Fit.**
- At a 1280 × 900 window, the summary, the full chart card and at least five table rows are
  visible without scrolling.
- At shorter windows the page scrolls; the plot keeps its height and text keeps its size.
- No page-level horizontal scrollbar at any width of 360 px or more.

## 4. The comparison chart

### 4.1 Encoding

**R-7. Card header.**
- Title: `CPU by container` or `Memory by container`.
- Controls, in order:
  1. `SegmentedControl` CPU | Memory. URL param `metric`, values `cpu` or `memory`, default `cpu`.
  2. The existing `HistoryControls`: ranges 15m, 1h, 24h, 7d and 30d, default 15m. URL
     params `range` and `end` are unchanged.

**R-8. Axes.**
- Plot area height: 240 px.
- X axis: the selected range, positioned by real timestamps (`time_ms`), with 4 tick
  labels (start, two interior, end). In Live the end label reads "now".
- Y axis starts at 0.
- Y ceiling: `niceCeiling` / `nextCeiling` (`src/core/chart-math/nice-ceiling.ts`, through
  `useNiceCeiling`) over the largest plotted value of all charted lines, with the existing
  60 s shrink hysteresis. `minCeiling` is 10 (percent) for CPU and 64 MiB for memory.
- Neither engine capacity nor any container's limit sets the ceiling.
- One Y axis only. No stacking, no per-line normalization, no smoothing (straight segments).

**R-9. Units.**
- CPU is percent of one core: 100% means one fully used core, and values can exceed 100%.
  Y caption: `% of one core`.
- Memory is working-set bytes, formatted with the user's unit preference. Y caption: `Working set`.

**R-10. Project scope.**
- A select above the chart. Options: `All projects`, then each distinct `project` in the
  snapshot sorted by name. Containers with no project are listed under `Standalone`.
- URL param `project`; absent means all projects.
- The scope applies to the summary (R-4), the chart candidates (R-14) and the table (R-29).
- It never changes the monitored set.
- The selected project stays visible in the control. Choosing `All projects` clears it.

### 4.2 What a point means and how it is drawn

R-11 and R-12 depend on the slice B wire change (R-46). Slice A ships with today's
`covered` flag, read as R-12a describes.

**R-11. Drawing points.** Each history point carries a measured duration and a covered
flag per metric (R-46). For the selected metric:

| Point | How it is drawn |
|---|---|
| `measured_ms == 0`, or value is `null` | No data. Leave a gap. |
| `covered == true` | Draw the value. Connect it to the previous point if that point was also covered and `prev.time_ms + prev.duration_ms == time_ms`. |
| `measured_ms > 0` and `covered == false` (partial bucket) | Draw the value as an isolated dot, never connected to neighbors. |
| Covered, but no connectable neighbor on either side | Draw as a dot. |

**R-12. Which value to plot.**
- `duration_ms <= 10_000`: plot the bucket average (`metrics`).
- `duration_ms > 10_000` (rolled-up tiers and server downsampling): plot the bucket peak
  (`peak`, R-46) so short spikes stay visible. The Y caption gains ` · peak per {duration}`,
  for example `% of one core · peak per 1 min`.

**R-12a. Slice A only.**
- Until R-46 lands, treat a point as covered when `measured_ms >= expected_ms * 0.98`.
- Compute this in one exported helper in `src/core/chart-math/series-lines.ts`, with a
  comment pointing to R-46.
- Plot `metrics` at every duration.
- Slice B deletes this helper.

**R-13. Marks.**
- Lines: 2 px, round joins and caps.
- Dots: 6 px diameter.
- Color: `var(--color-series-N)` for the container's slot N (R-17).
- Gridlines use the recessive grid style of neighboring charts.
- Text never uses series colors.

### 4.3 Which containers are charted

**R-14. Candidates.** Containers in the project scope with data for the selected metric in
the range.
- Slice A: snapshot containers in scope whose current value for the metric is non-null
  (running and measured now).
- Slice C: every container with at least one stored or ring point in the range, including
  stopped and removed ones, as returned by the comparison read (R-50).

**R-15. Automatic mode (default).**
- If there are 6 or fewer candidates, chart all of them.
- If there are more, chart the top 5, ranked descending by:
  - Slice A: the current value from the snapshot.
  - Slice C: the peak of the selected metric over the range, from R-50.
- Ties break by container id. Missing values never rank as zero; they rank last.

**R-16. Automatic re-ranking.**
- Recompute membership immediately when the metric, range, `end` or project scope changes.
- In Live, re-evaluate each time the snapshot's observed time crosses a 10 s boundary.
- A non-charted candidate replaces the lowest-ranked charted container only after ranking
  above it on 3 consecutive evaluations.
- A pinned or focused container (R-22) is never replaced; skip it and replace the
  next-lowest instead.
- Show no notice, toast or text when membership changes.

**R-17. Slots.**
- There are six slots. On first entry, assign slots in rank order (rank 1 gets slot 1).
- A container keeps its slot while it stays charted, across metric switches, range changes
  and re-ranking.
- An entrant takes the lowest free slot.
- Sorting never reassigns slots. Two charted containers never share a slot.

**R-18. Custom mode.**
- The chooser (R-19) switches to custom mode when the user changes the charted set.
- Custom membership is stored in URL param `chart` as comma-separated container ids in slot
  order.
- It survives metric, range and scope changes and Back navigation, and is never re-ranked.
- Containers in `chart` that fall outside the current project scope are not drawn, but stay
  in the param.
- `Use top 5` in the chooser deletes the `chart` param and returns to automatic mode.

**R-19. The chooser.**
- The legend's last item is a button labelled `+N more` when candidates outnumber charted
  lines (N = candidates − charted), and `Choose…` otherwise.
- Its tooltip reads `Top 5 by {current|peak} {CPU|memory}` in automatic mode with more than
  6 candidates, and `Custom selection` in custom mode.
- It opens a `Dialog` (`src/app/components/ui/dialog.tsx`) titled `Show on chart`, containing:
  - a search field;
  - a scrolling list (max height 352 px) of all candidates sorted as in R-15. Each row has a
    checkbox, the slot swatch (empty if not charted), the name, the project and the current
    value;
  - a count `{k} of 6`;
  - a `Use top 5` button, and `Done`.
- Changes apply immediately.
- Checking a seventh container is disabled, with the inline text
  `Remove a container to compare another.`
- These checkboxes affect the chart only and never call `dockerConfigure`.

**R-20.** No aggregate line, no "Other" line, and no subtraction from totals. Every line is
one container.

**R-21. Identity.**
- Identity is `(engine id, container id)`. Names are labels only.
- A recreated container with the same name is a different line.
- When two charted lines share a name, both legend entries append the first 6 characters of
  the id.

### 4.4 Interaction

**R-22. Focus emphasis.**
- Hovering or focusing a legend item, a table row (R-31) or a line focuses that container:
  its line draws at 3 px and the others at 30% opacity.
- Clicking a legend item pins the focus; Escape or a second click unpins it.
- Focus never changes membership, navigation or collection.

**R-23. Crosshair.**
- Moving the pointer over the plot moves a vertical crosshair to the nearest point time.
- A tooltip (`ChartTooltipShell`) lists every charted container, sorted by value
  descending: swatch, name, value with unit, and `partial` for a partial point or `no data`.
- For points with `duration_ms > 10_000`, show `min / avg / max` and the bucket interval
  (`11:20–11:21`), never an exact time for the peak.

**R-24. Pin and open.**
- Clicking the plot pins the crosshair.
- In the pinned tooltip, container names become links to the container detail route
  (`containerPath` in `_lib/presentation.ts`) with the same engine and metric, `range=15m`,
  and `end` = bucket end + 5 minutes.
- Escape or clicking outside unpins.

**R-25. Keyboard.**
- The plot is focusable: `tabIndex=0`, `role="img"`, and an aria-label naming the metric,
  the range and the charted containers.
- Left/Right move the crosshair one point.
- Up/Down move focus between charted containers.
- Enter opens the focused container's detail as in R-24.
- Escape unpins.

**R-26. "Show as table".**
- A text button under the legend replaces the plot with a table.
- One row per point time (newest first), one column per charted container.
- Values with units, `—` for no data, and a `partial` suffix for partial points.
- Max height 240 px; the table scrolls.

**R-27. Brush (slice C).**
- Dragging across the plot selects a historical interval, using the existing
  `BrushProvider`, `BrushOverlay` and `src/core/brush.ts`.
- `BrushOverlay` was built for live ring charts. If it cannot take a history range, extend
  it with the props it needs; do not write a second brush.
- A selection switches the table to historical mode (R-36).
- `Live` clears the selection and the pin.

## 5. The table

**R-28. Structure.** Replace `ContainerTable`'s paging and checkbox.
- Delete `ContainerMonitor` and the `page` URL param.
- The body has a max height of 352 px (8 rows × 44 px), scrolls vertically with a visible
  scrollbar, and has sticky column headings. With fewer than 8 rows it is as tall as its rows.
- Search, state filter, View menu and row count sit above the scroller.
- With more than 100 rows, virtualize with `@tanstack/react-virtual`, as
  `src/app/components/process-table.tsx` does. Otherwise render every row.
- Restore the table's scroll offset on Back, using `history.state`.

**R-29. Rows.**
- Every container in the project scope that matches the search and the state filter.
- The state filter default stays `All states`.
- Text search and the state filter affect only the table, with the count
  `N matching containers`. They never change the chart or the summary.

**R-30. Columns**, in order. In slice A, omit the action column.

| Column | Content | Width rule |
|---|---|---|
| (swatch) | 8 px slot swatch if the container is charted, empty otherwise | 20 px |
| Name / project | name, then ` · project` in muted text | flexible |
| State | lifecycle label plus health (existing `statusLabel` / `healthLabel`) | content |
| CPU | live `cpu_pct`; `—` when null | right aligned |
| Memory | `working_set` when there is no explicit limit; `{used} / {limit}` with a 48 px bar when `explicit_limit_bytes` is set | right aligned |
| Net | `network_rx_bps + network_tx_bps` as a rate; `—` when null | right aligned; hidden when the table is narrower than 720 px (container query) |
| (action) | row action (R-37) | 72 px |

- Block I/O, image, ports, restarts and PIDs are optional columns in the View menu (keep the
  existing `columns` URL param).
- Never print "No explicit limit", "No check" or an engine-capacity denominator in a row.

**R-31. Row and line linkage.**
- Hovering or focusing a row emphasizes that container's line (R-22), if it is charted.
- Focusing a line (from the legend or the keyboard) gives its row the existing selected-row
  background, and scrolls it into view only if it is out of view.

**R-32. Sorting and grouping.**
- Default sort: descending by the selected metric (CPU or Memory).
- Switching the metric changes the sort, unless the user has clicked a column heading in
  this route session.
- Clicking a heading sorts by that column (existing `sortedContainers`, ties by id) and
  shows a sort indicator.
- The View menu keeps `Group by project` (existing `group` param). Grouping changes only the
  table, and group headings count toward the 352 px.

**R-33.** Stopped, exited and removed rows show `—` for live metrics, never `0`.

**R-34. Held order.**
- While the pointer is over the table or a row has keyboard focus, row order does not
  change; values keep updating.
- Order resumes on pointer leave or blur. No text is shown.

**R-35. Health.**
- Unhealthy, OOM-killed and repeated-restart rows show the existing dated health text in
  the State column.
- If such a row is hidden by the search or state filter, the table header shows a link
  `{n} need attention`. It clears both filters and scrolls to the first affected row.
- When no row is affected, show nothing.

**R-36. Historical mode (slice C).** Active when a brush selection exists, or `end` is set
(not Live).
- Table title: `Containers · {start}–{end}`.
- Rows come from the comparison read's ranking (R-50) and include removed containers.
- CPU and Memory columns become `Peak CPU` and `Peak memory`. Peaks are never summed.
- The State column shows the last lifecycle observed in the range, or `Unknown`.
- The Net column is hidden.
- The summary line keeps showing live values.
- Row actions (R-37) reflect each container's current state, and removed containers have none.

### 5.1 Row Start/Stop (owner decision, 2026-10-13)

**R-37. Row action.** Each row for a container that exists now has one action in the action
column.
- It is revealed on row hover or focus, with the same visual pattern as `ProcessRowActions`
  (`src/app/components/process-actions.tsx:62`):
  `opacity-0 group-hover:opacity-100 group-focus-within:opacity-100`.
- The same action is also in a right-click `ContextMenu` (`src/app/components/ui/context-menu.tsx`).

| Container state | Action | Confirmation |
|---|---|---|
| `running`, `restarting`, `paused` | `Stop` | Yes. `ConfirmDialog` titled `Stop {name}?`, body `Docker sends SIGTERM and waits 30 seconds, then kills the container.`, confirm variant `destructive`. |
| `exited`, `created` | `Start` | No. Dispatch immediately. |
| `removed`, `removing`, `dead`, `unknown` | none | none |

**R-38. Disabled actions.**
- Stop is disabled, with a tooltip giving the reason, when the container has AutoRemove or
  unknown host config. This is the D-107 rule: stopping such a container can delete it and
  its anonymous volumes.
- Both actions are disabled while the engine is not `connected`.

**R-39. In-flight and results.**
- While an action is in flight, the cell shows `Stopping…` or `Starting…` until the snapshot
  reports the new lifecycle or the operation result arrives.
- Results use the existing outcome kinds: success, skipped, failed, outcome unknown.
- A failure shows its reason in the row, under the name, for 10 s, and in a toast
  (`sonner`), for example `Port 5432 is already in use`.
- Actions are never retried automatically.
- The row does not move while its action is in flight (R-34 applies).
- The detail page (`detail-route.tsx`) shows the same action next to its title, with the
  same rules.
- Chart, legend, chooser, scope and filter interactions never start or stop anything.

## 6. Monitoring settings and notices

**R-40. Monitoring dialog.** `Monitoring…` in the PageHeader replaces the `Engine…` button
and opens a `Dialog` titled `Monitoring`. Its sections, in order:

1. Engine: the existing `EngineChooser` content.
2. `Automatic collection`: explanatory text, with no per-container selection controls.
   - The selected Docker daemon is the source of truth for provisioned containers.
     Inventory and running collection membership follow daemon changes automatically.
   - Collect metrics from running containers, up to the engine's active collection cap
     (128). Stopped containers do not consume active collection slots; a newly running
     container joins automatically when a slot is available.
   - Containers that stop or are removed keep their recorded history. The chart chooser
     changes only the compared lines and never changes collection membership.
   - Remove `Choose containers`, monitoring checkboxes/search, selected-count text,
     staged `Apply`/`Cancel`, and the manual `monitored_containers` / `explicit_selection`
     settings. Previous saved subsets no longer restrict monitoring.
3. `Show in popover` and `Show count in menu bar` checkboxes, moved from the footer and saved
   immediately as today.
4. `Disable monitoring` button, moved from the footer, with its existing behavior, and
   `Done` to close the dialog.

This automatic inventory behavior supersedes the original manual-selection design by
explicit owner request on 2026-10-13.

**R-41. The notice line.**
- The chart card shows at most one line of notice text plus a `Details ({n})` button.
  When everything is healthy, it shows nothing.
- The text comes from the first matching row below. `n` counts every matching row.

| Priority | Condition (source) | Text |
|---|---|---|
| 1 | `snapshot.status` is `unavailable`, `permission_denied` or `incompatible` | `{engine} unavailable · last reading {time}` with `Retry`. `{time}` is the latest `observed_ms` across containers in the last snapshot. |
| 2 | `snapshot.status` is `paused` or `idle_suspended` | `Monitoring paused` with `Resume` (the existing resume action) |
| 3 | `persistence.status == "write_failed"` | `Recent history is not being saved. Kelvo will retry.` |
| 4 | Any non-event coverage record in range for the selected metric, or with no metric | `{CPU|Memory} history missing for {k} intervals` |
| 5 | `aggregate.partial`, or `aggregate.expected < aggregate.running` | `Monitoring {expected} of {running} running containers` with `Monitoring…` (opens R-40) |
| 6 | Coverage records with `events_disconnected` or `event_replay_uncertain` | `Some container events may be missing` |

- The population notice compares current running coverage: Rust's `aggregate.expected`
  counts running containers admitted for collection; `aggregate.running` counts all
  currently running containers. Inventory totals and stopped containers are not the
  numerator. For example, 200 running containers at the 128 active collection cap means
  `Monitoring 128 of 200 running containers`; the Monitoring dialog explains the cap.
- If priorities 1 and 3 both match, the text is `{engine} unavailable; recent history not saved`.
- The line may wrap at narrow widths, but never becomes a list.

**R-42. History details.** `Details ({n})` opens a `Dialog` titled `History details`.
- Groups: Collection, Saving, Metrics, Events. Show only non-empty groups.
- Each row shows the time range, the scope (container name, or `All containers`), the
  plain-language reason from the table below, and the raw reason code in muted monospace.
- The list scrolls, with a max height of 8 rows (352 px).
- Merge records with the same reason and scope whose spans touch or overlap; a merged row
  counts once.
- Never merge event records with metric records.

| Reason code | Plain language |
|---|---|
| `write_failed` | `Measurements could not be saved` |
| `write_rejected` | `History storage was full or the disk was low` |
| `writer_disconnected` | `History writer stopped` |
| `events_disconnected` | `Lost connection to Docker events` |
| `event_replay_uncertain` | `Some events may be missing after reconnecting` |
| `app_not_running` | `Kelvo was not running` |
| any other | the raw code |

**R-43. Gaps on the chart.**
- Engine-wide metric records (`container_id == null`) draw the existing `GapBand` over the plot.
- Container-scoped records break only that container's line.
- Event records never draw on the plot.

## 7. Backend changes

### 7.1 Slice B: history fixes

**R-44. Fix Bug 2 at the source.** Free a Docker batch's admission slot when the batch has
been applied to the open transaction, not at commit.
- In `handle_docker` (`writer/docker.rs`), on a successful apply, under the admission lock:
  - decrement `pending_writes`;
  - subtract the batch's `bytes` from `pending_bytes`;
  - push a slim record onto `docker_pending` that keeps only `reply`, `generation` and
    `revision`. Do not keep the batch body until commit.
- `finish_docker` must not decrement the counters again for these slim records. It still
  decrements for batches that finish before apply, such as rejections inside `handle_docker`.
- The acknowledgement contract is unchanged: `Persisted` is sent only after COMMIT.
- This also removes the double count in the allowance check, because `reserved` no longer
  contains applied batches.

**R-45. A `write_failed` record means measurements were permanently dropped.** Nothing else
may create one.

Store side:
- The store no longer records losses, because the engine retries everything a rejected
  batch carried (below).
- Remove `Loss`, `Admission.lost`, `Admission::lose`, `docker_write_losses`,
  `docker_recorded_losses`, the `loss_scope_overflow` path, and the docker `lost` handling
  in `writer/discard.rs` that exists only for them.
- Keep the ack outcomes `Rejected` and `Failed`.

Engine side:
- In `drain` (`persistence.rs:1555`), on `Rejected` or `Failed`:
  - set `s.error` as today;
  - do not call `gap(.., "write_failed")`;
  - requeue the receipt's non-bucket content. `Receipt` keeps the events and coverage rows
    its batch carried; on failure, push them to a new `s.retry` (events, coverage) that
    `batch()` drains into the next batch.
- Bound `s.retry.events` at 4096. When older events are dropped, append one
  `event_replay_uncertain` coverage row for the dropped span to `s.retry.coverage`.
- In `evict` (`persistence.rs:1671`), before removing an entry whose `bucket.revision > durable`:
  - append a container-scoped coverage row with reason `write_failed` and span
    `[bucket_ms, bucket_ms + 10_000)` to `s.retry.coverage`;
  - the entity is the container id, or `None` for the aggregate entity;
  - merge contiguous spans for the same container.
  - This is the only producer of `write_failed`.
- Log every `Rejected` or `Failed` ack with `tracing::warn!`, including the reason, rate
  limited to once per 60 s per engine.

**R-46. Fix Bug 1 and publish per-metric quality.**

Change `DockerHistoryPoint` (`crates/kelvo-schema/src/docker.rs:199`):
- Remove `measured_ms` and `covered`.
- Add `cpu_measured_ms: u32`, `memory_measured_ms: u32`, `cpu_covered: bool` and
  `memory_covered: bool`.
- Add `minimum: DockerMetrics`: per-bucket minimum of each metric, from stored `min`;
  `null` where the metric has no weight (owner clarification during implementation, 2026-10-13).
- Add `peak: DockerMetrics`: the per-bucket max of each metric, from the stored `max`;
  `null` where the metric has no weight.
- Keep `expected_ms` and `duration_ms`.

Change `point()`. For each of CPU (`cpu_percent`) and memory (`memory_working_set`):
- `measured` is that metric's `weight_ms`, or 0 if it is absent.
- `covered = measured > 0 && measured + expected / 50 >= expected`.
- The tolerance is 2% of the bucket: 200 ms at 10 s, 1.2 s at 1 min, 18 s at 15 min.
- Put `expected / 50` in a named constant, with a comment citing this RFC's Bug 1 measurement.

Update the `point()` unit test (`persistence.rs:1995-2017`):
- 10000, 9999 and 9800 are covered.
- 9799 and 0 are not covered.
- Full CPU weight with memory weight 0 gives `cpu_covered = true` and `memory_covered = false`.

Then:
- Regenerate bindings (`make bindings`).
- Update `dockerHistory` in `src/core/mock/docker.ts` to emit the new fields (see R-52).
- Delete the slice A helper (R-12a) and switch the chart to R-11 and R-12.

**R-47. Clean up existing false records.** Add schema migration V7: append to `MIGRATIONS`
in `crates/kelvo-store/src/db.rs` and set `SCHEMA_VERSION = 7`. It runs:

```sql
DELETE FROM docker_coverage
WHERE reason IN ('write_failed', 'loss_scope_overflow') AND entity_id IS NULL;
```

Why this is safe enough: every such row was written by the admission path that R-45
removes, and the measurement in §1.1 shows they overwhelmingly cover data that was saved.
A genuine engine-wide loss recorded before this fix is lost as metadata; that is accepted
under the pre-v1 policy in `CLAUDE.md`.

### 7.2 Slice C: comparison read and container actions

**R-48.** Before implementing R-49, record decision D-108 in `plan/decisions.md`:
"Container rows get Start and Stop. Stop keeps the D-107 safeguards: 30 s grace, AutoRemove
and unknown host config refused, one operation per engine, revalidate before dispatch, no
automatic retry. Start is non-destructive and dispatches without confirmation under the
same operation runner."

**R-49. Container action command.**

Interface:
- Add Tauri command `docker_container_action` in `src-tauri/src/docker.rs`. It takes
  `{engine_id: String, container_id: String, action: DockerContainerAction}` and returns
  `DockerStorageOperation`.
- Define `enum DockerContainerAction { Start, Stop }` in `crates/kelvo-schema`.
- Reuse the `DockerStorageOperation` type and `dockerStorageOperation` polling.

Engine implementation:
- Inspect only that container; do not run a storage scan.
- Refuse with a `Skipped` item and a plain-language message when:
  - the engine id or generation changed, or the container is gone;
  - Stop is ineligible: it requires lifecycle `running`, `restarting` or `paused`, known host
    config, and `AutoRemove == false`;
  - Start is ineligible: it requires lifecycle `exited` or `created`.
- Dispatch through the same one-operation-per-engine runner D-107 uses:
  - Stop calls `stop_container` with `t: Some(30)`, as in `docker_storage.rs:463-472`.
  - Start calls `start_container`, which is new, next to it in
    `crates/kelvo-collect/src/docker_storage.rs`.
- Never retry. Report success, skipped, failed or outcome unknown, as D-107 does.

Frontend:
- Add `dockerContainerAction` to the `Transport` interface (`src/core/transport.ts`) and its
  Tauri implementation.
- Add it to the mock (`src/core/mock/docker.ts`): flip the mock container's lifecycle after
  500 ms. The mock container named `worker` fails Start with `Port 8080 is already in use`.

**R-50. Comparison read.** Add Tauri command `docker_comparison` and the transport method
`dockerComparison(DockerComparisonRequest) → DockerComparisonPage`:

```rust
pub struct DockerComparisonRequest {
    pub engine_id: String,
    pub from_ms: i64,
    pub to_ms: i64,
    pub metric: DockerComparisonMetric,     // Cpu | Memory
    pub project: Option<String>,            // None = all; Some("") = Standalone
    pub selected: Option<Vec<String>>,      // container ids, max 6; None = automatic (R-15)
    pub max_points: u32,                    // per series, max 360
}
pub struct DockerComparisonPage {
    pub generation: u32,
    pub ranking: Vec<DockerRankedContainer>,   // every container with data in range, max 500
    pub series: Vec<DockerContainerSeries>,    // the charted set
    pub coverage: Vec<DockerCoverage>,
    pub persistence: DockerPersistence,
}
pub struct DockerRankedContainer {
    pub container: DockerContainer,            // latest metadata, including removed containers
    pub peak_cpu_pct: Option<f64>,
    pub avg_cpu_pct: Option<f64>,
    pub peak_working_set_bytes: Option<f64>,
    pub avg_working_set_bytes: Option<f64>,
    pub cpu_measured_ms: u64,
    pub memory_measured_ms: u64,
    pub last_lifecycle: Option<DockerLifecycle>,
}
pub struct DockerContainerSeries {
    pub container: DockerContainer, // retained metadata independent of ranking cutoff
    pub points: Vec<DockerHistoryPoint>,
}
```

Ranking:
- All entities of kind `container` with any bucket in `[from_ms, to_ms)`, from the ring and
  the store.
- Read in one consistent snapshot, reusing the existing ring-plus-store join that
  `docker_history` uses.
- Sorted by the selected metric's peak, descending; ties by id; nulls last.
- Peaks come from the stored `max`; averages from the weighted `sum / weight`.
- Truncate at 500 rows. The UI shows the first 500.

Series:
- With `selected`: those containers.
- With `None`: every ranked container if there are 6 or fewer, otherwise the top 5.

Bounds: one call returns at most 6 series × `max_points` points. Reject larger requests
with a typed error.

Decision record: write one for these types only if `plan/architecture.md` classifies them
as a one-way door. Its one-way doors are the series data model, host identity and the
remote sync wire format; Tauri command types are regenerated bindings.

Mock: implement `dockerComparison` in `src/core/mock/docker.ts` from the same synthetic
generator as `dockerHistory`. Include two removed containers that have history only in the
first half of any range.

**R-51.** In slice C, the chart and the table switch from per-container `dockerHistory`
calls (slice A) to one `dockerComparison` call, and R-14 and R-15 use their slice C
definitions.

### 7.3 Mock data (slice A, extended in B and C)

**R-52.** Update `src/core/mock/docker.ts` so the mock reproduces real data, not ideal waves.

Add scenario `docker-11`:
- 11 containers, 5 running, matching the owner's setup: `api`, `worker` and `caddy` in
  project `liveoak`; `grafana-dev` and `tempo` in `opendata`; the rest exited.
- Register it in the scenario regex in `createMockDocker` (`/^docker-(0|1|11|32|128|200)$/`)
  and in `src/core/mock/fixtures.ts`.

Per-container history (`container_id` set):
- Each container differs: different base levels and phases.
- Include one 60 s gap, two partial buckets (weight 7000), and weight 9999 on ordinary buckets.

Peaks:
- `peak` values exceed the averages by 5–40%.
- One 1-minute bucket has a peak 3× its average (the spike case).

## 8. Design-system amendments

Slice A updates `plan/design-system.md` and `src/app/styles/theme.css`.

**R-53. Ramp rule exception.** Add to `plan/design-system.md`: "Container comparison charts
use categorical series colors, one per container, because overlapping per-entity lines
cannot be told apart as ramp steps. Colors encode identity only, never health or metric."

**R-54. Series tokens.** Add these to `src/app/styles/theme.css`, in both the light block
and the dark block, in this order:

| Token | Light | Dark |
|---|---|---|
| `--color-series-1` (blue) | `#2a78d6` | `#3987e5` |
| `--color-series-2` (orange) | `#eb6834` | `#d95926` |
| `--color-series-3` (aqua) | `#1baf7a` | `#199e70` |
| `--color-series-4` (violet) | `#6250d6` | `#9085e9` |
| `--color-series-5` (magenta) | `#e87ba4` | `#d55181` |
| `--color-series-6` (green) | `#008300` | `#008300` |

Validation, run on 2026-10-13 with the dataviz palette validator against `--color-card`
(`#ffffff` light, `#111113` dark), on adjacent pairs:
- All checks pass in both themes.
- In light mode, aqua (2.82:1) and magenta (2.69:1) are below 3:1 against white. The legend
  and the R-26 table provide the required non-color path.
- Yellow and red are excluded because they collide with `--color-warning` and
  `--color-destructive`.
- Six colors cannot pass every-pair color-vision checks, so identity never relies on color
  alone: legend names, focus emphasis, table swatches and R-26 cover it.

**R-55. Dashes and gallery.**
- Dashed lines are not used for identity.
- Add the comparison chart, the chooser and the notice line to the dev gallery
  (`src/app/routes/dev/gallery/_sections/app-components.tsx`), with sample data in
  `src/app/routes/dev/gallery/_lib/chart-samples.ts`.
- The sample data has six lines, one partial dot, one gap and the spike.

## 9. Delivery slices

### Slice A: remove the traps and show named lines (frontend only)

Requirements:
- In scope: R-1 to R-26, R-28 to R-35, R-40 to R-43, R-52, and R-53 to R-55.
- R-14 and R-15 use their slice A definitions, and R-12a applies.
- Waiting for slice C: R-27, R-36, and R-37 to R-39 (omit the action column).

Tasks:

1. Tokens and design-system text (R-53, R-54).
2. `src/core/chart-math/series-lines.ts`: pure functions with no React imports.
   - `buildSeriesRuns(points, metric, from, to, width, height, ceiling)` returns line runs and
     isolated dots per R-11, using the R-12a helper in slice A.
   - `rankCandidates` and `nextMembership` implement R-15 to R-17 as pure state transitions.
   - Unit tests cover every case each of those requirements describes.
3. `_components/comparison-chart.tsx`: the SVG chart per R-7 to R-9, R-13 and R-22 to R-26.
   - Uses `useNiceCeiling`, `ChartTooltipShell` and `SeriesSwatch`
     (`src/app/components/charts/chart-tooltip.tsx`).
   - Data: one `useDockerHistory` call per charted container, at most 6. This is the only
     allowed per-container fan-out, and slice C replaces it.
4. `_components/chart-chooser.tsx` (R-19), and the legend with its `+N more` / `Choose…`
   button. If `Legend` (`src/app/widgets/legend.tsx`) cannot take arbitrary CSS colors, add a
   `color` prop to `LegendItem` rather than writing a second legend.
5. `_components/history-notice.tsx` and `_components/history-details.tsx` (R-41 to R-43).
   Remove the coverage paragraphs from `container-history.tsx`; that component stays in use
   only on the detail page.
6. `_components/monitoring-dialog.tsx` (R-40). Remove the footer and the `Engine…` button.
7. Rework `_components/container-table.tsx` (R-28 to R-35): delete `ContainerMonitor` and
   paging; add the swatch, Net and limit display, the bounded scroller, and held order
   without the paused bar.
8. Rework the `route.tsx` layout (R-1 to R-6, R-10). Delete `docker-activity-review.tsx`.
9. Mock: the `docker-11` scenario and realistic history (R-52).
10. Tests:
    - unit tests for task 2, and component tests for tasks 3 to 7;
    - update `route.test.tsx` and `_components/container-table.test.tsx`;
    - update `tests/e2e/docker-containers.spec.ts`. The population test at line 118 asserts
      "Now · N shown" and must change with the new table copy;
    - update the containers scenario in `tests/e2e/perf-gate.spec.ts` if its selectors change.
      Do not raise budgets.

Done criteria:
- `bun run check` and `bun run test:e2e` pass. `make check` passes once at the end.
- Dark and light screenshots at 1280 × 900 and 800 × 600 for `scenario=docker-11`,
  `docker-0`, `docker-1`, `docker-32`, `docker-128` and `docker-200`, compared against the
  CPU, Memory and Network pages and the dev gallery. Every remaining difference is named in
  `plan/PROGRESS.md`.
- The healthy `docker-11` screenshot shows nothing between the header and the table beyond
  what R-2 allows.
- On the owner's real OrbStack, the chart shows five named lines. They stay partly
  fragmented, and the notice line still reports false save failures, until slice B lands.
  Record this in `PROGRESS.md`.
- The task script (§10) has been run on the mock and on real OrbStack, with results in
  `PROGRESS.md`.

### Slice B: history fixes (Rust, parallel with A)

Requirements: R-44 to R-47.

Tasks:

1. R-44, in `crates/kelvo-store/src/writer/docker.rs` and `writer/txn.rs`.
   - Regression test in `crates/kelvo-store`, using real SQLite: with `commit_interval` at
     300 s, enqueue 400 small Docker batches over a simulated 299 s with no commit. All are
     admitted, and all receive `Persisted` after the commit.
   - Revert-verify: the test fails on the current code.
2. R-45, in the store and the engine. Tests, using the fake `Ticker` and clock per
   `.claude/rules/testing.md`:
   - a `Rejected` ack leaves no `write_failed` coverage, and the same buckets appear in the
     next batch;
   - events and coverage rows from a rejected batch appear in the next batch;
   - evicting a non-durable bucket produces exactly one container-scoped `write_failed` row
     with that bucket's span;
   - the warn log is rate limited.
3. R-46: the schema change, the `point()` change and its test, bindings, the mock fields,
   and the frontend switch from R-12a to R-11/R-12. Delete the R-12a helper.
4. R-47: the migration, plus a migration test. A V6 database with engine-wide and
   container-scoped `write_failed` rows keeps only the container-scoped rows after opening.

Done criteria:
- `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` pass.
- `make bindings` leaves no diff afterwards.
- `bun run check` passes, then `make check` once.
- `cargo test -p kelvo-engine --test perf_gates -- --nocapture` passes with no budget raised.
- Real-engine check:
  1. Install the build with `scripts/install.sh`.
  2. Keep Kelvo running with OrbStack for 30 minutes with the five containers up.
  3. Copy the database per Appendix A and run both scripts.
  - Pass when zero `docker_coverage` rows with reason `write_failed` were created after
    install, and for every running container at least 99% of stored 10 s buckets in the
    window are CPU-covered under the R-46 rule (Appendix A script 2, column `r46_cpu`).
  - Record the numbers in `PROGRESS.md`.

### Slice C: compare and investigate, plus Start/Stop

Requirements: R-27, R-36, R-37 to R-39, and R-48 to R-51. R-14 and R-15 switch to their
slice C definitions.

Tasks:
1. Record D-108 (R-48).
2. `docker_container_action` (R-49).
   - Rust tests use the existing storage adapter test fakes.
   - Extend the `#[ignore]` disposable-fixture test pattern (`docker_storage.rs:1600`) with
     `KELVO_DOCKER_TEST_START_ID`.
3. Row and detail actions (R-37 to R-39), with component tests.
4. `docker_comparison` (R-50), with store tests covering removed containers, a recreated
   same-name container, ties, nulls and the 6-series bound.
5. Switch the chart and the table to the comparison read (R-51).
6. Brush and historical table (R-27, R-36).

Done criteria:
- The suites listed for slice B pass.
- Screenshots of historical mode, a removed container's line, and both actions' in-flight
  and failure states.
- The disposable Start/Stop fixture test has been run once by hand against OrbStack, with
  the result recorded.
- The task script has been re-run.

### Slice D: finish

Requirements: everything above, verified end to end.

Tasks:
1. Fix what the task script and screenshots found.
2. If any 1 Hz render path changed, run `make bench` and report the measured numbers. Do not
   estimate them.
3. Update `plan/docker-container-monitoring.md` to point at this RFC for the overview design.
4. Mark this RFC Done in `plan/PROGRESS.md`.

## 10. Task script (run after slices A, C and D)

Run it on `scenario=docker-11` in the browser dev server and on the real app with OrbStack.
Record the time and pass or fail for each task in `PROGRESS.md`. A failed task changes the
design before the next slice.

1. Name the top CPU container within 10 s, and the top memory container within 10 s more,
   without opening Monitoring.
2. Match one chart line to its table row without reading the legend (by swatch or hover).
3. (Slice C and later) Find which container caused a marked spike earlier in the range,
   including one that has since been removed, within 30 s, and open its detail at that time.
4. Scope the page to `liveoak` and compare its two busiest containers.
5. Add a sixth container to the chart, then try a seventh. Read the limit message. Return to
   top 5.
6. Explain the difference between `Show on chart` and automatic engine monitoring.
   Create or start a fixture container and confirm it appears and starts recording without
   changing a saved selection; stop it and confirm its history remains.
7. From the default page plus one click on Details, tell apart measured zero, missing CPU
   history, missing events, and history not being saved (use mock states).
8. (Slice C and later) Stop a running container from its row and start it again without
   leaving the page. The row does not jump under the pointer.
9. When a container outside the top 5 becomes expensive in Live, notice it from the table or
   the `+N more` button. If this fails, open a follow-up for membership-change notices.

## 11. Open items

None blocking. Deferred:
- Membership-change notices, only if task 9 fails.
- Batch actions using the Processes selection surface.
- Restart, pause and remove actions.
- The ~5% missing buckets and the aggregate entity's zero-weight buckets from §1.1. Open a
  diagnostic task if slice B's real-engine check shows them in normal operation.

## Appendix A: real-history audit

Run these against a copy, never the live file. The scripts only read.

```bash
DB="$HOME/Library/Application Support/com.tryopendata.kelvo/history.sqlite"
OUT="$SCRATCH/history-copy.sqlite"          # any scratch directory
sqlite3 "file:$DB?mode=ro" ".backup $OUT"
```

Script 1 lists coverage records by reason, then the `write_failed` timeline:

```sql
SELECT reason, entity_id IS NULL AS engine_wide, count(*) AS n,
       sum(coalesce(end_ms, start_ms) - start_ms) / 1000 AS seconds
FROM docker_coverage GROUP BY 1, 2 ORDER BY n DESC;

SELECT strftime('%H:%M:%S', start_ms / 1000, 'unixepoch', 'localtime') AS start,
       strftime('%H:%M:%S', end_ms / 1000, 'unixepoch', 'localtime') AS end,
       (end_ms - start_ms) / 1000 AS seconds
FROM docker_coverage WHERE reason = 'write_failed' ORDER BY start_ms;
```

Script 2 reports per-container bucket weights. Payloads are CBOR `[version, [metric…]]`.
Save it as `weights.py` and run `uv run --with cbor2 python -I weights.py "$OUT"`:

```python
import sqlite3, sys, collections, cbor2
db = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
names = dict(db.execute(
    "select e.id, coalesce((select name from docker_metadata m where m.entity_id = e.id "
    "order by effective_ms desc, revision desc limit 1), '(aggregate)') from docker_entities e"))
stats = collections.defaultdict(collections.Counter)
for entity, payload in db.execute("select entity_id, payload from docker_10s"):
    _, metrics = cbor2.loads(payload)
    w = {m["key"]: m["weight_ms"] for m in metrics}
    cpu, mem = w.get("cpu_percent", 0), w.get("memory_working_set", 0)
    s = stats[names.get(entity, entity)]
    s["n"] += 1
    s["strict_covered"] += min(cpu, mem) >= 10000          # current rule (Bug 1)
    s["cpu_covered_r46"] += cpu > 0 and cpu + 200 >= 10000  # R-46 rule
    s["short_by_le_100ms"] += 9900 <= min(cpu, mem) < 10000
for name, s in sorted(stats.items(), key=lambda kv: -kv[1]["n"]):
    n = s["n"]
    print(f"{name:24} buckets={n:6} strict={100*s['strict_covered']/n:5.1f}% "
          f"r46_cpu={100*s['cpu_covered_r46']/n:5.1f}% "
          f"short<=100ms={100*s['short_by_le_100ms']/n:5.1f}%")
```

Baseline on 2026-10-13, before any fix:
- strict coverage was 45–47% per container;
- 52–54% of buckets were short by 100 ms or less;
- 58 engine-wide `write_failed` rows covered 1.9 h of a 5 h window.

## Appendix B: change history

- 2026-10-13: first draft (aggregate-chart revision proposal, devil's-advocate review).
- 2026-10-13: conventions review against Docker Desktop, `docker stats`, Grafana and Datadog.
  Replaced bespoke membership notices and per-trace whiskers; added row swatches, project
  scope, Net column, explicit-limit memory, the status-copy budget and the scripted task check.
- 2026-10-13: owner decided rows get Start/Stop.
- 2026-10-13: real-database audit confirmed Bugs 1 and 2. Rewrote this document as an
  implementation spec with numbered requirements. Earlier drafts are fully superseded.

- 2026-10-13: owner superseded manual monitored-container selection. The selected daemon's
  actual provisioned/running containers now drive automatic inventory and collection,
  capped at 128 active running collectors. R-40 removes selection controls and settings;
  R-41 reports active running coverage and links to Monitoring. Chart selection stays
  independent. Previously saved manual subsets must not restrict collection.
