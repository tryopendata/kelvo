# History and remote monitoring landing plan

Status: Implemented and reviewed, 2026-10-24. Local and real-host evidence is below; final-tip hosted status is in the [main CI workflow](https://github.com/tryopendata/kelvo/actions/workflows/ci.yml?query=branch%3Amain) and task handoff. Native click-through and reboot acceptance remain open.

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
- [x] Use an explicit same-version agent update for the existing QA host, through
  the filtered `update_the_installed_agent` acceptance path. Current `0.1.0` builds
  do not offer an automatic update to another `0.1.0`. Do not add an update subsystem
  or bump a public release solely for this source landing.
- [ ] Test the native Update offer on a disposable host with an older-version
  fixture.
- [x] Verify the saved unit and effective systemd properties:
  `MemoryHigh=134217728`, `MemoryMax=268435456`, `MemorySwapMax=0`. Confirm daemon
  reload/restart applies them without a task-owned runtime override.
- [ ] Verify that reboot preserves the saved limits. Reboot authorization has
  not arrived; leave the designated QA server running.
- [x] Complete a full `/` space scan under those persisted limits while remote
  detail is visible. Record scan completion, sampler responsiveness, restart count,
  cgroup `memory.current`, anon/file/slab from `memory.stat`, swap, pressure,
  `memory.events`, and process `smaps_rollup` before, during and after report release.
- [x] Repeat scan/cancel/release and reconnect cycles, then observe after the
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

- [x] Capture fresh light/dark screenshots of the affected modules, custom range
  controls, long/fixed charts and gallery. Compare with the design system and
  neighboring screens, including narrow layouts, keyboard use and reduced motion.
  Record each intentional difference.
- [ ] Verify the picker in native WKWebView too. Native UI automation timed out;
  browser WebKit evidence does not complete this check.
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


### Warm Linux working-set correction (2026-10-24)

The late warm check disproved the 96M soft-limit acceptance: after more than
three hours the agent held about 104 MB of anonymous memory and no file cache.
A full root scan reached a full-memory-pressure `avg10` of about 67%.
Raising only the runtime high limit to 128M let the active scan finish; a fresh
scan on the same PID completed in 33 seconds, with 29 distinct measured frames
and three successful cancel/release cycles. No hard-limit/OOM or swap event
occurred. These observations establish reclaim pressure, not a missed-sample
count or a proven long-term memory plateau.

Both generated units now use a 128M soft limit for working-set headroom; the 256M hard
backstop and zero swap are unchanged. Their two assertions fail against the old
96M templates and pass after the change. The complete remote crate suite passes.
The independent native reviewer approved the templates and the opt-in acceptance
helper. D-116 distinguishes summary-only footprint measurements from the warm
detail workload.

Both static musl agents were rebuilt and passed ELF alignment checks. The normal
installed-agent update preserved host `570924aa-dc29-4105-9ef2-7df7ec34d84f`, epoch
`63c4ee6e-4b36-4aad-8c11-6305e16db53d` and history. The exact task-owned runtime
MemoryHigh override was removed; saved and effective limits are now 128M / 256M /
zero swap with only vendor drop-ins remaining. Continuous detail demand through
the full retention hour, another full scan and repeated post-release observations
are still running. The acceptance helper checks frame/query progress; its printed
PID, memory, swap/OOM counters and PSI deltas must be assessed separately.

Hosted run `40148227337` at `d85447b` passed all 430 functional browser checks on
both Ubuntu and macOS, native/Linux suites and bindings. The Ubuntu run had two
recovered first-attempt timeouts; macOS functional cases all passed first try.
The macOS performance step failed broad ScriptDuration/long-task checks and hit
the existing 600-second suite cap after retries. The local normal push hook had
passed all ten unchanged gates. A fresh-runner retry and performance review are
underway; the hosted gate is not accepted merely from local results.


The multi-controller acceptance review found the same counted-demand defect in
`AgentHost` that had already been repaired in the app shell. Repeated merged
`detail=true` values incremented the engine counter, so disconnecting controllers
could leave detail collection active. The agent now forwards only aggregate
boolean transitions under the existing interest lock. A real-engine regression
covers repeated requests, overlapping sessions, unrelated process interest,
local/remote overlap and the final release back to background mode. Removing
the guard fails with two demands instead of one; the restored agent suite passes
all 19 unit tests. The test waits for the following engine command before checking
the asynchronously published background status.

Both agent binaries were rebuilt with that repair, and the normal QA update again
preserved identity, epoch and history. The previous partial soak was intentionally
interrupted and retained only as diagnostic evidence. A fresh full retention-hour
run started at 01:53 UTC on October 25 against the corrected aarch64 binary,
with the saved 128M / 256M / zero-swap unit and no task-owned runtime override.

A separate temporary diagnostic branch ran unchanged Containers, Overview and
CPU 1h gates in baseline/candidate/candidate/baseline order on one hosted runner.
Both baseline Containers samples failed (45.75 / 60.27 ms/s), as did the improved
candidate (43.87 / 39.15). CPU varied within both revisions, so these samples do
not establish a systematic new range regression. Provisioning and Spotlight work
were present at sample boundaries, but that alone does not explain each failure.
The diagnostic retains every nonzero gate result separately from collection-job
success; it changes no production assertions or budgets.

Candidate profiles identified chart geometry, paths and legend work repeating at
1 Hz while the displayed Containers history range advances every 10 seconds.
Derived series and geometry now retain their identity when their inputs are
unchanged; the existing line and legend subtrees are cached at those boundaries.
Controls and notices still receive live updates. The real-component/transport
regression verifies idle history while live table values change, then verifies
bucket, hover, pin and metric changes. It fails on the old rendering path and
also fails when legend caching is deliberately invalidated. Review approved the
complete cache inputs. All 136 Containers unit/integration tests and all 120
Chromium/WebKit Docker checks passed, including custom lines, brush interactions,
coverage, tooltip and resize behavior. Full performance measurement is pending.

The performance helper also accepted reloads during warm-up without repeating
readiness or the full warm-up, and returned its third sample even if that sample
spanned another reload. Two real-browser regressions reproduce these failures:
one accepted only about three seconds of warm-up, and repeated reloads returned
a result instead of rejecting it. The helper now checks document identity across
readiness, warm-up and measurement, retries readiness after a reload, and rejects
three invalid attempts. Both regressions pass after the fix. Budgets, sample
lengths, retries and suite caps are unchanged. CI prints read-only runner load and
process diagnostics before performance measurement; this does not establish that
runner contention caused any particular failed sample.

The complete local check and bindings gate passed with 1,071 Rust tests (48
ignored) and 1,276 frontend tests in 195 files. Final dark/light Containers and
gallery captures match existing typography, surfaces, spacing and chart styling;
there are no intentional visual differences from caching the existing subtrees.

The final local performance run passed all 12 cases, including both reload
regressions and all ten existing gates. Containers measured 32.66 ms/s, CPU 1h
26.64, GPU 1h 26.95 and Power 1h 33.42, with no long tasks in those samples.
Overview measured 33.79 ms/s; Performance mode measured 16.05 against 34.07
plain. Opening and hidden-window resume also passed. The helper discarded and
repeated a sample that spanned a reload. The separate final typecheck passed.

### Completed warm-agent acceptance and Back-link correction

The corrected installed agent completed 3,600 seconds of continuous detail
demand on one PID, with continuing usage queries and 3,548 distinct measured
frame timestamps observed by polling. Root scans at 20 and 40 minutes completed
in 19 and 20 seconds. The final warm scan completed in 20 seconds with 18 observed
frames, followed by a fresh scan in 21 seconds with 19 observed frames. Each scan
had three successful cancel/release cycles. The final sampled anonymous peak was
61 MB and the cgroup peak was 136.3 MB. All restart, swap, maximum-limit and OOM
counters remained zero. The saved/effective unit remained 128M / 256M / zero swap,
with only vendor drop-ins and the same installed binary hash.

Twenty-five additional memory/smaps observations span the last warm minutes and
both final scans. The largest observed full-PSI delta was 2.36% over about ten
seconds. These diagnostics establish responsiveness during this workload; neither
the polling frame count nor the passing query assertions establish uninterrupted
1 Hz cadence or a long-term memory plateau.

Eleven further observations over ten minutes after the final report release
ranged from 53.85 to 54.15 MB anonymous memory, with no restart, swap, hard-limit
or OOM event. File cache and slab are recorded separately; total cgroup memory
is not used as an anonymous-leak estimate. All diagnostic clients have finished.

The production app rebuilt from `5d84c1c` passed deep/strict signature validation
and system-library dependency checks. Both packaged agents match their built
bytes and embedded SHA-256 strings; the aarch64 bytes match the installed QA
agent. This is the pre-Back bundle; verify its replacement and retain the source
revision, packaged-resource hashes and signature/dependency checks in the final
task handoff.

Hosted run [40155045096](https://github.com/tryopendata/kelvo/actions/runs/40155045096)
at `5d84c1c` passed Linux, license/source/banned-dependency checks and all 430 Ubuntu
functional browser cases, with two first-attempt recoveries. macOS passed native,
bindings and functional checks, then failed frontend performance checks despite
valid warm-up periods. A fresh-runner diagnostic runs the unchanged full gate
twice without native or functional workloads to test runner isolation. Existing dependency advisories remain advisory; no audit was
disabled or ignored. The final task handoff records the verified tip and cleanup.

The Disk recovery exposed a real navigation race: the first historical process
link already contained its selected interval, but its Back URL briefly lacked
that interval while the parent mirrored brush state into the URL. Activating that
actual first link returned to Live. Back is now derived synchronously from current
brush state, preserving device/response parameters and removing stale bounds for
live links. Both real-component regressions fail on the original source and all
47 Disk tests pass with the fix. The browser's immediate-first-link navigation
passed three repeats afterward. Independent review approved; a Space hover
recovery passed five focused WebKit repeats without an established product fault.

The Back correction passed the full check and bindings gate: 1,071 Rust tests
(48 ignored), 1,278 frontend tests in 196 files, type checks, Clippy, architecture
fixtures and binding generation. The final full functional browser sweep and
normal push performance gate follow this source verification.


The first complete Back-fix browser run passed 428 of 430 cases and exposed two
fixture defects. The Space clock was installed but continued advancing with real
wall time, so the eight-second mock scan could finish before its partial-stage
assertion. The test now pauses explicitly, advances to a label-bearing partial
stage, and resumes after completion so folder-query notifications can run. The
motion classifier required an effective duration of exactly 150 ms, although CSS
shortens interrupted reversals. It now permits positive effective durations up to
150 ms only when the declared token remains 150 ms, preserving the existing
property, element and iteration guards.

A real CSS-reversal regression fails with the old classifier in both engines. It
also rejects a 400 ms effect, a nominal 400 ms transition shortened below 150 ms,
and a replayed entrance. Removing either duration guard fails both browser
projects; both guards were restored. All 20 affected Space/motion cases pass,
and independent reviewers approved both fixture corrections. No production
motion, timeout, assertion or performance threshold was relaxed.


The corrected complete functional suite passes all 432 Chromium/WebKit cases
with two workers and no retries. Fresh-runner diagnostic
[40157532387](https://github.com/tryopendata/kelvo/actions/runs/40157532387)
ran the exact `5d84c1c` gate twice without native/functional workloads. Both full
runs failed: Power in the first, then Containers/Power/open/resume in the second.
Runner isolation alone is insufficient and is not adopted as the resolution.
The raw results, unchanged assertions, status codes and new profiles are retained
in the task evidence directory for the next performance investigation.


### Production performance measurement

The exact-code profiles show React development JSX self time of 357 ms per
20 seconds in Containers, 270 ms in Overview and 176 ms in CPU. They do not
identify one shared redundant production computation. D-060 already names a
production-build Playwright target as a reason to change the measurement. The
reviewed target builds production React plus the explicit browser mock into
`target/perf-dist`, starts a fresh preview, and verifies production mode, route
and theme. Native packaging continues to use its separate `dist`; functional
browser tests retain development mode.

This changes measurement runtime and increases headroom under the same numeric
ceilings. Historical development numbers are not comparable. The fixture size,
cadence, routes, themes, viewports, budgets, sampling, long-task checks, retries
and suite cap remain the same. Component/transport render regressions continue
to catch narrower selector and caching mistakes.

In sequential local runs, baseline `6cccd17` with the common harness and the
candidate each passed all 13 cases. Main-thread ms/s, baseline / candidate:

| Screen | Baseline | Candidate |
| --- | ---: | ---: |
| Containers | 11.35 | 9.30 |
| Container detail | 5.80 | 5.45 |
| Popover | 12.23 | 12.19 |
| Overview | 12.63 | 11.17 |
| CPU 1h | 9.38 | 9.89 |
| GPU 1h | 12.10 | 11.10 |
| Power 1h | 11.21 | 9.16 |

Neither run reported long tasks; opening, hidden resume and the reload guards
also passed. These single pairs do not establish a precise speedup. A deliberate
mutation drove actual StreamArea reconstruction through React on every animation
frame rather than the sampling cadence. CPU measured 112.50 ms/s and failed its
existing 35 ms/s ceiling. No artificial delay was inserted. The original source
was restored byte-for-byte; the normal final push gate verifies it again.
Together with the comparison, this establishes the production target as a gross
regression guard, with the changed headroom explicitly accepted in D-060.

The remaining concrete Power finding was an overly broad battery-history
subscription: the card received changing system watts, battery watts and
temperature although it renders none of those values. Its hook now selects only
the displayed battery summary. The real-transport Profiler regression fails on
the old hook, keeps commits unchanged through power-only frames, then verifies
visible charge and history-query updates. Review approved; all 18 affected
component/hook/Power tests pass. No markup or styling changed.


After the final subscription/startup changes, all 78 affected Chromium/WebKit
browser cases pass, plus the two development-runtime checks and all 18 focused
battery/Power tests. Fresh WebKit Power/Battery history and gallery captures in
both themes retain the existing typography, accents, surfaces and spacing.
There are no intentional visible differences from the subscription change.
The final normal push runs the complete code, bindings and restored production
performance gates; its exact revision and hosted result belong in the task handoff.
