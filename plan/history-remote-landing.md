# History and remote monitoring landing plan

Status: Implemented and verified locally, 2026-10-24. Main integration and hosted CI are recorded in the task handoff. Native click-through and reboot acceptance remain open.

Finish the module time ranges, Linux agent memory limits and remote feed lifecycle,
verify the combined app, land the commits on `main`, and remove only this batch's
temporary resources. The range and lifecycle review findings have been repaired.

## Starting state and scope

- The primary checkout is on `fix/agent-memory-limits` at `6cccd17`, one commit
  ahead of local `main` (`42518ce`). That commit contains the memory-limit fix.
- The uncommitted module-range work spans 47 tracked files and 11 new files.
  Include the untracked hooks, helpers, components and tests in review and staging.
- Remote `main` was checked directly at `26fda11`. Local `main` contains that
  commit and is 36 commits ahead, with no divergence at planning time. Landing
  this batch will also publish those existing local commits. Check again before
  integration; the cached tracking ref alone is insufficient.
- There is one worktree, the primary checkout. Keep it and its build caches.
- Use separate commits for the range feature, lifecycle repair, and final acceptance
  record. Preserve `6cccd17`; do not recreate its changes on `main`.

This batch ends with verified source on local and remote `main`. Publishing a new
release, notarization, widgets, general SSH architecture refactoring and unrelated
old QA items remain outside its scope. The existing macOS and Linux CI jobs must
pass for the landed revision; fixing failures that block those jobs is in scope.

The plan applies `ce:writing-tests` and `ce:architecting-systems`, with repository
rules taking precedence. Three read-only reviews used the CE code-reviewer
instructions. History and UI reviewers ran 14 and 34 scoped frontend tests
respectively; all passed. Those passes do not cover the findings below. The agent
and lifecycle review was static. Full gates and native acceptance remain open.

## 1 Preserve the work and establish the baseline

- [x] Record status, branch, commit IDs and the task-owned changed/new file list.
  Keep a recoverable copy of the working diff and the new files outside the
  checkout before integration. A patch alone does not preserve untracked files.
- [x] Confirm no other session is editing this batch. Keep changes in the current
  branch through verification; no additional worktree is needed for implementation.
- [x] Check dependencies and Git hooks. Use the existing toolchain versions and
  `make hooks`; do not bypass commit or push hooks.
- [x] Preserve an unchanged `6cccd17` comparison build for the Power diagnosis.
  If a temporary checkout is needed, isolate its build outputs, run `bun install`,
  and record its ownership for cleanup. Run performance samples sequentially.

## 2 Finish module time ranges

Resolve these review findings within the existing range implementation. Keep Rust
responsible for aggregation, coverage and gaps. Reuse the existing transport,
range-query and brush boundaries; no new history engine or dependency is needed.

| Fix | Required behavior and regression |
| --- | --- |
| Sampling fallback in `src/core/live-window.ts:60` | Custom relative durations must receive the same temporary sampling safeguard as presets. Apply a two-minute duration, slow sampling to 60 seconds, verify an allowed effective window, then restore sampling and verify the saved two minutes returns. Absolute dates stay fixed. |
| Plot bounds in CPU `total-card.tsx:149` and equivalent GPU, Memory and Power callers | Ticks, plotted timestamps, gap placement and pointer/keyboard brushing must use the same plot domain. Derive historical line bounds from the returned grid's actual first/last plotted timestamps; keep the requested dates for summary queries. Test both edges and a nonaligned start. Never add a measurement to make an endpoint match. |
| Fixed Disk bounds in `disk/route.tsx:120` | A brush inside fixed dates scopes totals and processes without shifting either surrounding plot or its query bounds. Fixed chart bounds take precedence over the brushed endpoint. Preserve the existing rolling-range behavior. |
| Disk placeholders in `disk/route.tsx:131` | Hold previous answers only while a rolling window advances. Delay a same-duration fixed B response after fixed A and require loading instead of A's bars or totals under B's dates. |
| Disk Live action in `disk/route.tsx:242` | Its pressed state must reflect fixed versus rolling range as well as the brush. Activation clears selection and saves a rolling range of the same duration. A subsequent live frame must advance the chart. |
| Historical formatting in `selection-summary.tsx`, Disk charts and `live-mirror-chart.tsx` | Multi-day axes, selections and hover readouts must distinguish dates. Fixed mirrored charts must announce selected dates and must not describe their final closed bar as "so far." Test equal clock times on different days and the final fixed bar. |

- [x] Add behavior regressions through real components and the mock transport;
  use deferred responses to exercise range and host changes while a read is pending.
  Cover both typed command errors and rejected transport promises, recovery, empty
  history, partial retention, and gaps without showing zero or another host's data.
- [x] Exercise every affected module at a short live range, long rolling range and
  fixed dates. Extend the current CPU-only long/fixed browser flow to Disk and
  representative Network and Power interactions, in Chromium and WebKit.
- [x] Verify range persistence across routes and window settings mirrors, Escape
  dismissal, validation, timezone/date labels, and brush reset on fixed-date changes.
- [x] Keep the real SQLite 90-day regression covering point/cell budgets, weighted
  averages, extrema and gaps. Verify returned bucket origins at retention-tier
  boundaries and on populated heatmaps; do not rely only on the browser fixture.
- [x] Revert-verify each new regression by reintroducing its defect and observing
  the intended failure, then restoring the fix. Do not mock internal modules.
- [x] Regenerate bindings with `make bindings`. Confirm the generated diff follows
  the Rust `ChartWindow` change, then run frontend typecheck and tests. Run codec
  round-trip/skew coverage for the changed schema; no protocol version change is
  planned unless the contract actually requires one.

## 3 Resolve the Power performance blocker

The recorded candidate repeatedly produced 54–56 ms tasks against the 50 ms gate,
while unchanged HEAD passed in the same session. Its cause is unresolved.

- [x] Capture candidate and unchanged `6cccd17` traces on the same quiet machine,
  using the existing Power scenario and unchanged budgets. Include the 10-second
  Energy by app refresh coinciding with a sample. Stop other task-owned builds
  before measuring; leave other sessions' processes alone.
- [x] Identify the expensive commit or computation from the traces. Change the
  responsible selector, query scheduling or rendering only when the trace supports
  it. Memoizing usage rows is a hypothesis, not a predetermined fix.
- [x] Add a deterministic render-count or query-lifecycle regression at the observed
  boundary. Cover disabled history observers during live ticks and fixed history
  during unrelated live updates. Revert-verify the test.
- [x] Pass the Power scenario and the full `make e2e-perf` gate. Separately diagnose
  the recorded native allocation/IOKit failures using `perf_gates -- --nocapture`
  and same-session baseline evidence. A retry alone or a budget increase does not
  resolve a candidate regression.

## 4 Repair remote feed ownership and demand

Keep this repair in the app shell's registry and registration lifecycle. No React
reload workaround, wire-format change or collector change is required.

- [x] Define the feed identity for interest accounting as the registered source
  instance, not just its stable host ID. On source replacement, invalidate the old
  host's slots across every window, abort their streams, release old demand and
  clear the cached process/network/GPU/port aggregate. Wire this into registration,
  discard, rollback and re-pair paths. Retain monotonically increasing subscription
  generations so catch-up already in progress cannot reinstall an invalidated slot.
- [x] Make resubscription install the current feed and apply its detail interest
  before initial status/backfill. Reapply identical process views to the new source;
  the old host-keyed cache must not suppress them. Verify the existing host-change
  event triggers frontend resubscription for an unchanged selected host ID; add
  source-generation invalidation at that boundary if it currently does not.
- [x] Aggregate detail demand across visible windows in `LiveRegistry`, then send
  the merged boolean to `HostEntry`. Hiding one window must retain detail while
  another needs it. Update the additive fake to follow the production contract.
- [x] Test through a real `HostEntry` with a recording `SourceControl`, using actual
  hubs/channels. Cover A-to-B replacement under one host ID, old-feed release,
  new-source frames and process rows, identical flags, hidden replacement, repeated
  replacement, two windows, window destruction and host removal.
- [x] Cover registration rollback, re-pair's disabled old host, and invalidation
  during interleaved catch-up using the existing subscription-race seam. Confirm
  no obsolete stream can win. Revert-verify the new regression cases.

Start with these scoped suites, then expand according to the repository ladder:

```sh
cargo test -p kelvo live::tests
cargo test -p kelvo remote::setup::linux::tests
cargo test -p kelvo-remote agent_layout
cargo test -p kelvo-remote --lib agent_install::tests
cargo test -p kelvo-agent commands::tests
```

## 5 Complete agent and native acceptance

The memory-limit literals in `6cccd17` received no code-review findings. Acceptance
must prove the installed service uses the new unit, not just temporary runtime
properties. Preserve the QA host's identity and history.

- [x] Run `make agent-linux`, then build the app with the new agents. Assert both
  packaged `Contents/Resources/agent/kelvo-agent-<triple>` resources exist and match
  the built bytes and compiled digests. `build-app.sh` can succeed without agents
  when pinned build tools are missing, so its exit status is insufficient.
- [ ] Use an explicit same-version agent update for the existing QA host, through
  the filtered `update_the_installed_agent` acceptance path. Current `0.1.0` builds
  do not offer an automatic update to another `0.1.0`. Do not add an update subsystem
  or bump a public release solely for this source landing. Test the native Update
  offer on a disposable host with an older-version fixture.
- [ ] Verify the saved unit and effective systemd properties:
  `MemoryHigh=100663296`, `MemoryMax=268435456`, `MemorySwapMax=0`. Confirm daemon
  reload/restart applies them and that reboot preserves them.
- [x] Complete a full `/` space scan under those persisted limits while remote
  detail is visible. Record scan completion, sampler responsiveness, restart count,
  cgroup `memory.current`, anon/file/slab from `memory.stat`, swap, pressure,
  `memory.events`, and process `smaps_rollup` before, during and after report release.
- [ ] Repeat scan/cancel/release and reconnect cycles, then observe after the
  in-memory hour fills. Account for the earlier 81 MB resident plus 36 MB swap
  observation. Establish bounded retained anonymous memory rather than treating
  kernel-charged cache as a leak; fix unbounded growth or reclaim stalls if found.
- [ ] In the built app, verify Add host, update, edit with an open dashboard,
  re-pair, remove, remote history after disconnection, brush attribution and Space.
  Verify reconnect after a scheduled reboot of the designated QA machine. Use a
  disposable target for clean-install, identity-reset and purge workflows.
- [x] Run only named ignored real-host tests with an explicit target. Never run
  blanket `--ignored` against the installed M1 agent: lifecycle tests install,
  reset identity and purge their target. Collect the memory diagnostics around
  the filtered `queries_answer_on_a_real_host` scan if reusing that test.
- [x] Run `make rust-linux-test` and `make remote-scripts-test` after
  `ensure-docker`. Check the actual hosted Linux result after pushing, including
  portable tests, both musl builds, smoke/pipe tests and the store fill test.

## 6 Review and verify the final candidate

- [ ] Capture fresh light/dark screenshots of the affected modules, custom range
  controls, long/fixed charts and gallery. Compare with the design system and
  neighboring screens, including narrow layouts, keyboard use and reduced motion.
  Record each intentional difference. Verify the picker in native WKWebView too.
- [x] Dispatch fresh CE code-reviewer passes over the final range diff and native
  lifecycle diff, including all new files and regressions. Resolve each finding or
  disprove it with evidence. Earlier reviews are findings to address, not approval.
- [x] Once fixes stop changing, run `make check`, `bun run test:e2e`,
  `bun run build`, and `make e2e-perf`. Use the focused test ladder while iterating;
  do not repeatedly launch full suites without a change or unresolved failure.
- [x] Record exact commands, nonzero test counts, commit/revision coverage, native
  results and benchmark evidence. Any changed code after verification gets its
  affected checks rerun. Keep failed evidence and its resolution in the record.

## 7 Commit land and clean up

1. Prepare the progress/design/acceptance records with the completed checks and
   reconcile this batch's stale summary rows in `PROGRESS.md` and `plan/README.md`.
   Preserve dated history. Commit the completed range feature, including Rust settings, generated bindings,
   new files, tests, design-system changes and its existing progress entry. Commit
   the lifecycle repair separately with its regressions. Keep any independent Power
   repair separate when it can stand alone. Finish with the acceptance/plan record.
   Use explicit files or hunks; preserve unrelated work and inspect what hooks stage
   or return to the working tree. Check `git status --porcelain` after each commit.
2. Run `make bindings-check` after committing the generated output; this target
   compares against the commit and cannot pass while that output is still dirty.
   Re-review `git diff main...HEAD` and commit contents for completeness.
3. Refresh remote refs and check ancestry again. If `main` remains an ancestor of
   the finished branch, switch to `main` and fast-forward it with
   `git merge --ff-only fix/agent-memory-limits`. If upstream advanced, integrate
   it on the task branch and reverify the resulting revision before landing.
4. Push `main` normally. Its existing pre-push hook runs
   `make check bindings-check e2e-perf`; allow that required repeat. Do not force
   push, skip hooks or treat a failed push as landed. Inspect all required hosted
   CI results for the pushed SHA and repair any failures before closing the batch.
5. Record the landed SHA and CI links in the final handoff. Mark only completed
   work as accepted; a local green run cannot close the hosted CI item. If CI
   requires another fix, commit it with its evidence update and verify the new tip.
6. Confirm local `HEAD` and remote `main` match and the checkout is on `main` with
   no remaining task changes. Delete `fix/agent-memory-limits` only after its commits
   are contained in verified `main`. Delete a remote task branch only if this batch
   created/pushed one. Leave the other existing branches alone.
7. Stop only task-owned temporary servers and diagnostics; remove their temporary
   comparison checkout and files after retaining useful evidence. Preserve the
   primary checkout, normal build caches, app data, QA host installation/history and
   bundled build outputs. Do not run `make clean`, broad `git clean`, or delete
   unrelated worktrees/branches. Remove the safety copy after verifying inclusion.

Completion means the reviewed changes are committed on local and remote `main`,
required checks pass at the landed revision, the installed QA agent passes the
memory and recovery checks, and no task-owned temporary resources remain.


## Execution evidence, 2026-10-24

The range and native reviewers approved the final source, including the subsequent
picker accessibility fix and acceptance-test changes. Eleven deliberate defect
mutations produced the expected regression failures; sources were restored. The
new picker Axe check also failed before its contrast repair.

- Range fixes cover custom sampling fallback, returned-grid bounds, fixed Disk
  brushing and Live, calendar labels, fixed/rolling placeholder isolation, and
  current endpoints for one-point GPU summaries. Fixed views no longer subscribe
  to unrelated live clock changes.
- Source replacement invalidates old subscriptions and interest caches. Retired
  feeds reject racing subscriptions with the existing retryable command error;
  frontend host-change events reconnect paused and display-idle remote views too.
  Visible-window demand is aggregated before reaching the source. The registry
  extraction reduced the legacy `live.rs` production ceiling from 1,050 to 750.
- Power traces showed the Energy by app refresh coinciding with chart work in both
  comparison builds. They did not establish a candidate-only cause. Memoizing the
  unchanged usage body and disabling fixed clock observers removes measured
  redundant rendering; deterministic regressions cover both boundaries. A focused
  Power gate passed at 28 ms/s with no task above 50 ms. Budgets are unchanged.
- `make check` passed: 1,069 Rust tests, 48 ignored hardware/manual scenarios,
  194 frontend files and 1,273 frontend tests, plus guidance/hooks/architecture
  fixtures, formatting, Clippy, App Store checks, lint and typechecking. All seven
  active native performance gates passed. The separate architecture gate and its
  26 fixtures passed after the ceiling change.
- `make rust-linux-test` and `make remote-scripts-test` passed. The Linux release
  fill/read suite reproduced the old hosted small-cap failure, then passed both
  tests after correcting its seven-day retention assumption: the documented cap
  protects the last day and can trim quarters followed by older minutes. The
  150 MB budget, 111 MB small cap and per-prune checks are unchanged. Measured
  uncapped sizes were 90.2 MB / 115.9 MB for 150 / 250 series; the capped case was
  105.5 MB. Independent disk-limit tests verify chronological cuts and the day floor.
- The first full browser run passed 428 cases and caught two light-mode picker
  contrast failures. The picker now uses the same ink token as the existing
  segmented controls; open-menu and selected-trigger Axe checks cover both themes.
  A separate WebKit checkbox test exposed asynchronous setup before Disk I/O
  loaded; waiting for the chart passed 20 repetitions. Its geometry assertions
  are unchanged. A later full run encountered two unexpected page reloads in
  unchanged Docker Storage scenarios; all 12 isolated repetitions passed. A further
  high-concurrency run exposed different timing failures. With the same two workers
  as CI, all 430 Chromium/WebKit cases passed in 4.8 min; Vite diagnostics recorded
  no file-change reloads. No assertion was removed or threshold relaxed.
- Compared all six affected module pages in light/dark, 90-day CPU, custom fields
  and the gallery. Also checked a 760 px picker with reduced motion and Escape.
  Intentional differences: the two-column preset/custom popover, the historical
  note distinguishing live headlines and partial attribution, and calendar labels.
  Line endpoints follow returned sample timestamps; bars cover their bucket
  intervals. Existing accents, surfaces, type, spacing and gaps are retained.
- Built both static musl agents and the production `.app`. Packaged resources match
  the built bytes and SHA-256 digests: aarch64 `f41a94b505f770130a1a7667f521f9b0d3177679b55393a89fc9877ccf18cc48`;
  x86_64 `1107d4d4dfaac590fe075ef090d14ece40c3901195f09734c215177dc012abe9`.
  The ad-hoc signature verifies; app and bridge link only macOS system libraries.
- Updated the existing `linux-tailscale` / m1-linux installation through the named
  same-version update test. Identity `570924aa-dc29-4105-9ef2-7df7ec34d84f`, epoch
  `63c4ee6e-4b36-4aad-8c11-6305e16db53d` and history survived. Saved and effective
  limits are 96M / 256M / zero swap. A summary-mode root scan finished in 21 s;
  a second scan with detail active finished in 63 s with 55 distinct measured live
  frames. Each measured 1,370,267 items / 135.5 GB. Three subsequent cancel/release
  cycles returned Idle. Peak anonymous memory during detail scanning was 93 MB;
  cgroup peak was 105.9 MB. Memory-high reclaim occurred; no max-limit hits, OOM,
  swap or restarts were observed. The separate real-host refill test recovered
  362 earlier measured rows, oldest 3,597 s before opening, at a 10 s widest step.

Remaining device acceptance is explicit: macOS UI automation timed out, so no
built-app Add host/update/edit/re-pair/remove click-through or native picker check
is claimed. No permission to reboot m1-linux has arrived; the host has not been
rebooted. Disposable-target older-version UI update and notarization are also
unverified. The installed service and its history remain intact. These limitations
must remain visible in the final handoff; automated tests do not close them.


Final local verification: `bun run test:e2e -- --grep-invert 'performance gate'
--workers=2` passed all 430 checks. `make e2e-perf` passed all ten scenarios in
4.2 min: Power was 23.35 ms/s, CPU 19.05, GPU 16.71 and Containers 33.39, with no
long tasks in those samples. The rebuilt final app again passed signature and
packaged-agent byte checks. No whole-app idle CPU improvement is claimed.

Implementation commits: range feature `e7e5f3a`, usage rendering `817535c`, source
lifecycle `1e3a992`, acceptance/test repairs `e267c05`, following memory limits
`6cccd17`. The required pre-push repeat, final hosted run, post-update hour
observation, main SHA and task-owned cleanup are reported in the task handoff;
this pre-push record does not claim those future results. Build/diagnostic evidence
is retained locally under `target/qa/history-remote-landing/` after cleanup.

The first required pre-push repeat passed `make check` and bindings but blocked
the push on Containers at 41.73 ms/s (35 ms/s limit). The same-session earlier
source `6cccd17` also failed at 40.10 ms/s. CPU traces showed unchanged filter
controls and the idle cleanup dialog rebuilding on metric updates. Memoizing the
filters by all input values and retaining the unchanged cleanup controller result
removed that work. A new public-consumer render regression failed against the
original hook and passed with the fix, including a changed-operation update.
The reviewer approved the change. The unchanged Containers gate then passed at
33.92 ms/s with no long tasks; no budget or sampling setting changed. The normal
pre-push repeat and hosted checks still determine whether this follow-up lands.

The post-update observation completed 66 samples over 65 minutes with the same PID, no service restarts,
no swap, no max-limit hits and no OOM events. After the detail scan, anonymous
memory was 85.6 MB at minute 25, 89.6 MB at minute 42, and 93.4 MB at minute 63;
it was unchanged for the last several samples. At minute 63, RSS was 96,336 KiB
and cgroup file pages were 1.4 MB. Memory-high reclaim occurred during scanning;
the final pressure averages were zero. This observation confirms bounded operation
over that interval; it does not identify the cause of the earlier resident-plus-
swap report or establish a longer-term plateau.

Follow-up frontend verification passed Biome, all TypeScript projects, 194 Vitest
files / 1,274 tests, and all 120 affected Chromium/WebKit Docker checks, including
filters, grouping, Stop, volume confirmation and inventory refresh.

A subsequent full native run exposed test contention for the process-wide four-
reader Docker snapshot limit: independent temporary-database scenarios could
exhaust it and fail at snapshot creation. The affected scenarios now share a
test-only mutex. Every snapshot/catalog caller in that test binary is covered;
the reviewer approved the isolation. Twenty full-binary runs with `write-stats`
and 32 test threads passed. Production admission limits and assertions are
unchanged. The rebuilt app's signature, system-library links and packaged-agent
digests also passed verification after the frontend follow-up.

The final shared `make check` passed after these fixes, including all 1,069 Rust
tests (48 ignored scenarios) and all 1,274 frontend tests. The normal pre-push
repeat and hosted checks remain part of the final handoff.

Main was fast-forwarded and pushed normally at `28c8177`; its required pre-push
repeat passed. Hosted run `40142083460` then found two remaining gate failures:

- The 510-container comparison expired its two-second SQLite snapshot. A same-
  session native sample showed repeated retained-range scans and SQL preparation
  for each candidate. Successful bounds are now cached per immutable WAL snapshot,
  and repeated history statements use rusqlite's bounded statement cache. The
  five-request debug fixture dropped from 3.51 to 2.17 seconds. The two-second
  expiry, four-reader admission and response bounds are unchanged. A real SQLite
  test covers empty/populated bounds across commits, reset and fresh snapshots;
  deliberately making the cache global failed that test. The catalog query moved
  into `docker/read/catalog.rs` to preserve the 500-line file limit. Native review
  approved the cache changes.
- Three narrow Disk geometry assertions failed on Ubuntu, while completed
  screenshots fit the viewport. Ten repetitions of the original case passed in
  the matching Linux Chromium container, so a persistent CSS defect is not
  established. The assertions now poll for the same strict zero document/content
  overflow, reporting excess pixels while layout settles. All 22 local Disk
  checks passed. The 430-case combined browser invocation also hit its existing
  ten-minute suite cap with 149 cases unrun; CI now invokes Chromium and WebKit
  separately, preserving that cap per invocation, individual timeouts, assertions
  and job limits. The reviewer approved; hosted completion remains required.

The hosted advisory audits also report existing WebPKI/Tokio runtime and
Happy DOM/Vite tooling dependencies. Their affected versions were already present
on previous remote main `26fda11`. The independent reachability review found no
established landing blocker, but dependency updates remain follow-up work.
The repository's tracking-issue policy is not satisfied by local triage alone;
a concrete draft and audit logs are retained with QA evidence, pending permission
to publish the separate public issue. Audit jobs remain enabled.

The hosted-failure follow-up passed the full local `make check`: 1,070 Rust tests
(48 ignored scenarios), all 1,274 frontend tests, type checks, Clippy and
architecture checks. The reviewer also approved the unchanged catalog extraction.
The normal push repeat and the next hosted run still need to complete.

Hosted follow-up `40144587371` at `8c67e81` passed Linux and native tests,
including the comparison that previously expired its snapshot. Ubuntu Chromium
passed, but WebKit disproved the readiness-only Disk explanation: both narrow
cases still had persistent document overflow despite a fitting main scrollport.
Linux WebKit reproduced the cause. Absolutely positioned `sr-only` volume-table
headings escaped static scrolling ancestors after entrance transforms ended.
Making the two volume scrollers relative containing blocks removes that overflow
without changing visible layout. Reduced-motion versions of the existing narrow
cases fail before the fix and pass afterward, while keeping the same zero-
overflow and accessibility assertions.

The light picker contrast failure in that run came from stale-state dimming,
confirmed by `main[data-stale]` in the trace and its blended foreground/background
colors. The accessibility flows had frozen mock ticks long enough for the stale
watchdog to fire on hosted WebKit. They now keep frames running while Axe checks
the live state. The selected ink token and production stale behavior are unchanged.
All three targeted Linux WebKit checks passed after these changes; review approved.

WebKit completed 205 cases before its existing ten-minute cap, with three failed
cases, two retry recoveries and five cases unrun. Its invocation is now split with
Playwright's standard `--shard` flag. Listing proves 108 and 107 disjoint cases
cover the complete 215-case project. Individual timeouts, assertions, retries,
suite caps and job limits remain unchanged. The next hosted run must complete.

The same hosted macOS run exposed stale dimming in the frozen Docker Storage
accessibility fixture. That flow now also keeps live frames. Local follow-up
passed all 1,274 frontend tests, all 50 affected range/Disk/gallery browser checks,
all eight Disk Space checks and all four Docker Storage accessibility checks.
The two scroller classes introduce no intended visible design differences.

Compared current Disk/Volumes, Space and gallery screenshots in both themes.
Typography, accents, surfaces, spacing and horizontal table scrolling match the
existing design; there are no intentional visible design differences. The narrow
Disk document remains exactly 360 px wide after motion settles. Captures are in
`target/qa/history-remote-landing/screenshots/containment-*`.

The next normal push was blocked by Containers at 35.41 ms/s; a focused repeat
was 36.97 ms/s, both without long tasks. A fresh CPU profile identified the
unchanged Activity/Storage switcher rebuilding Radix and measuring layout on
metric-only updates. Its existing JSX is now cached by selected view and the
Router callback. A real-transport regression observed three layout reads per
metric tick before the fix and none afterward, while both view transitions still
update and remeasure. All 14 route tests pass. Review approved the complete memo
dependencies, hook order and navigation behavior. The unchanged focused gate
then passed at 32.69 ms/s with no long tasks; the normal push repeat remains
required. No table rows, action guards or performance budgets changed.

The switcher follow-up passed the full frontend check: 194 files / 1,275 tests,
type checks and lint. All 120 affected Docker Chromium/WebKit checks passed.
Current light/dark storage screenshots preserve the existing layout and styling;
there are no visible design differences from caching the unchanged subtree.
