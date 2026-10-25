# Agent operations post-mortem, 2026-10-25

Scope: the history ranges, remote feed ownership and Linux agent memory batch
landed at `8cab4af`. This review applies `ce:post-mortem` to the execution record,
final handoff, local verification logs and exact-revision CI. It addresses repo
workflow gaps; it does not reopen the accepted product scope.

## What happened

The batch reached main with independent native/frontend reviews, defect regressions,
432 functional browser cases per hosted platform and 13 production performance
cases. [CI run 40160372209](https://github.com/tryopendata/kelvo/actions/runs/40160372209)
passed all required jobs. The real Linux agent completed a detail retention hour
with repeated scans under the accepted 128M high / 256M max / zero-swap limits.
Task-owned diagnostic resources were removed after preserving evidence.

The path there involved repeated full verification, ad-hoc extraction of successful
performance results, several fixture corrections and a late change from development
to production React measurement. Reviews found real lifecycle and navigation bugs;
recovered CI failures were not all machine noise. Native click-through, an older
version's UI update and reboot acceptance remain open.

## Execution assessment

- Accuracy improved when review findings became real-component or real-engine
  regressions. Paired baseline/candidate measurements and deliberate render mutations
  established what the performance gate actually catches.
- Efficiency suffered from split local/CI test selection, hidden successful hook
  output and completion evidence left in ignored files. Long CI waits also produced
  repeated status messages that added little beyond the existing run page.
- Ownership-aware cleanup worked: original tracked/untracked source was saved,
  managed archival was verified after its queued response, and unrelated resources
  were preserved. These steps should be reusable rather than reconstructed next time.

## Findings and changes

### 1. The normal browser command selected the wrong measurement lane

**What happened:** `make e2e-perf` correctly selected production React after D-060,
but bare `bun run test:e2e` still collected the performance file against development
React in both engines. CI separately excluded tests by a title substring. The
frontend testing rule still called the performance gate part of the default suite.

**Root cause:** runtime, suite selection and documented commands had separate owners.

**Closed, high priority:** `playwright.config.ts` now owns selection. Default runs
collect functional cases in both engines; performance mode selects only the gate
file with one Chromium worker. CI uses the same functional command. Real Playwright
collection regressions fail on the previous configuration and pass on the new one.
The production runtime, numerical ceilings and all performance assertions remain
as accepted in D-060.

### 2. A successful push hid the evidence needed to assess it

**What happened:** the normal pre-push hook displayed one buffered status line for
several minutes. Success suppressed the subprocess output. Recovering counts and
metrics required process inspection and unpacking Playwright's HTML report.

**Root cause:** pre-commit captured output without a repo-owned evidence destination.

**Closed, high priority:** `scripts/pre-push-check.sh` runs the existing three gates
sequentially, preserving live stdout/stderr, initial revision/status, stage exits
and final status in unique `target/qa/pre-push/` directories. The hook shows successful
output too. Three real Git/Make fixtures cover success, failure through `tee` and live
output before completion. Removing `pipefail` makes the failure regression go red;
restoring it passes. Later stages do not run after a failure. The wrapper changes
observability, not acceptance.
See [the evidence format](../scripts/verification/README.md).

### 3. A fresh agent could read a completed landing as unfinished

**What happened:** the tracked progress and landing records still said final push
and hosted acceptance were pending. The complete final handoff existed only under
ignored `target/qa/`, which ordinary `rg --files` also hides.

**Root cause:** completion relied on a chat/local artifact instead of the entrypoint
future sessions are instructed to read.

**Closed, high priority:** the tracked landing record now names `8cab4af`, its exact
successful CI run, actual counts, accepted hardware workload, remaining acceptance
and cleanup outcome. `PROGRESS.md` points to that result. This historical result is
distinct from verification of subsequent workflow/documentation changes.

### 4. Landing and cleanup decisions had to be reconstructed

**What happened:** exact-SHA CI lookup, required versus advisory jobs, ignored
evidence preservation and queued managed archival each required manual reasoning.
The final cleanup correctly checked both disk and Git registration before claiming
completion.

**Root cause:** the repo had a quality-check workflow but no focused landing/handoff
workflow. Repeated instructions in chat were not reusable guidance.

**Closed, medium priority:** [kelvo:land](../.claude/commands/kelvo:land.md) captures
the successful sequence, evidence requirements and ownership checks. It avoids a new
CI policy parser or custom worktree manager; GitHub, Git and Codex retain ownership.
It also distinguishes running the full gate once via the final push from immediately
duplicating the same work manually.

### 5. Browser readiness depended on undocumented timing details

**What happened:** installing Playwright's clock did not pause the Space scan;
pausing without resuming stranded React Query notifications. A visible Battery
heading did not mean its entrance motion and chart were ready for a screenshot.
Frozen live frames could also trip the stale-data watchdog during accessibility scans.

**Root cause:** timer replacement, application readiness and visual readiness were
treated as interchangeable.

**Closed as guidance, medium priority:** the existing frontend testing rule now
records explicit pause/advance/resume and content/motion readiness. The actual Space
and motion regression fixes already landed. A shared screenshot helper is deferred
until repeated call sites establish a common readiness contract; a heading-only
helper would encode the same mistake.

This review's first full browser invocation also reported two Chromium failures
with navigation during assertions and HMR activity while the checkout was being
edited. That is not stable-tree acceptance evidence. Both contexts/screenshots
were preserved; nine focused traced cases passed with no edits. The rule now
explicitly freezes config and documentation as well as application source during
acceptance. Those observations do not establish the cause of both failures.
A subsequent full run on an unchanged checkout passed all 432 functional cases
in 4.9 minutes without retries. Both the initial failure and stable-run output
are retained separately in `target/qa/agent-ops-post-mortem/`.

## Follow-ups retained

- **Linux acceptance tooling, medium:** a reusable real-host workload/evidence
  helper could replace the disposable soak scripts. It should capture saved and
  effective units, binary digest, PID, frame/query progress, anonymous memory,
  cgroup events, swap and PSI deltas across a full warm retention hour and scans.
  The warmed 96M failure shows why a cold summary-only footprint is insufficient.
  Do this with the next authorized hardware acceptance task, when the helper can
  be validated against the real workload. This post-mortem does not start a new
  soak or reboot the host.
- **Native acceptance:** retain the unverified built-app setup/update/edit/re-pair/
  remove/picker and reboot checks in the landing record. No browser or hosted VM
  result substitutes for them.
- **Dependency advisories:** the unchanged-lockfile findings remain owned under
  `.github/dependency-audits.md`. They are visible, advisory failures, not a reason
  to alter the required-check policy. The local issue draft was not published.

No personal plugin installation, global agent settings, new dependency, product
architecture refactor or expanded always-loaded agent policy is needed here. The
existing CE fallback lookup worked; guessed paths and repetitive status commentary
were execution mistakes, not evidence that another hook or policy is necessary.
