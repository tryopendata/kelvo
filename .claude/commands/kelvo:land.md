---
description: Land reviewed work with exact-commit evidence and task-owned cleanup
disable-model-invocation: true
---

Finish the requested batch on main, preserving unrelated work and recoverable evidence.
Use the existing task scope; this workflow does not authorize a release, public issue,
host reboot, identity reset or history deletion.

## Review and preserve

- Read `plan/PROGRESS.md` and the task's execution record. Record branch, HEAD,
  working changes, untracked source and resources created by this task. Preserve
  untracked source as well as the patch before integration or cleanup.
- Review the full batch, including new files. Fix findings and prove defect tests
  fail against the original behavior. Follow the scoped-test ladder while iterating.
- Inspect the staged diff before committing. After hooks, check Git status and
  confirm every intended file landed. Stage only task-owned paths when recovering
  hook edits; unrelated work may share the checkout.

## Verify and push

- Follow D-118: never dispatch or rerun a hosted macOS job, including an old CI or
  Release workflow. Inspect the workflow at the target revision before any rerun.
  Hosted checks use Linux only; native checks and release packaging stay local.

- Use the normal commit and push hooks. The pre-push hook runs
  `make check bindings-check e2e-perf`, saving live stage logs under
  `target/qa/pre-push/` ([evidence format](../../scripts/verification/README.md)).
  If the final normal push is the full gate, do not run it once manually and
  immediately repeat it unchanged through the hook. New edits or failures require
  the appropriate new checks.
- `bun run test:e2e` is the functional Chromium/WebKit lane. `make e2e-perf` is
  the production React, single-worker Chromium lane with macOS budgets. Avoid
  competing builds/benchmarks during performance measurement. Preserve a failing
  result and compare the same baseline/harness before attributing it to runner load.
- Confirm the pushed full SHA and select its CI run, rather than whichever main
  run is newest. With GitHub CLI, `gh run list --workflow ci.yml --commit <full-SHA>`
  finds candidates; `gh run view <run-id> --json headSha,status,conclusion,jobs,url`
  reports the actual revision and job state. Use the connected GitHub tools when
  available. Back off while unchanged; fetch completed job logs after completion.
- Check the required jobs defined in `.github/workflows/ci.yml`: frontend, Linux
  and dependency bans/licenses/sources. Advisory audit jobs and manual
  preview/hardware steps follow that file and `.github/dependency-audits.md`.
  Report advisory failures separately; do not disable them to make a run green.
- Record nonzero actual test counts, skips/retries, runtime and relevant metrics.
  A job's green wrapper is insufficient if the intended subprocess never ran.
  Hardware acceptance remains separate from browser mocks and hosted VM checks.

## Handoff and cleanup

- Update the tracked execution record and `plan/PROGRESS.md` with the landed source
  revision, exact CI link, observed acceptance and remaining checks. Local files in
  ignored `target/qa/` supplement that record; they must not be its only completion
  evidence. Label historical source verification explicitly so a later docs commit
  does not imply its own CI has already passed.
- Preserve needed ignored evidence and a recoverable source snapshot before
  removing a temporary checkout. Use the Codex managed archive lifecycle for managed
  worktrees. A queued archive is not completion: verify both the checkout's absence
  on disk and removal from `git worktree list` before reporting cleanup complete.
- Remove only task-owned branches, servers, temporary trees and logs. Identify
  processes by ownership, not broad name matches. Preserve shared caches, unrelated
  work, installations and history. Re-check Git status and worktrees at the end.
