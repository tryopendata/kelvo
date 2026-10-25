# Verification evidence

The Git pre-push hook runs `bash scripts/pre-push-check.sh`. It executes the same
three gates in order: `make check`, `make bindings-check`, `make e2e-perf`. It stops
at the first failure and preserves its exit code through `tee`. Successful hook
output is also shown by pre-commit's `verbose` setting.

Each invocation creates its own directory under `target/qa/pre-push/`:

| File | Meaning |
| --- | --- |
| `revision.txt` | HEAD at the start of the run |
| `working-tree.txt` | Initial Git status; nonempty means this was not a clean-commit check |
| `current-stage.txt` | Last stage started; this alone does not mean it finished |
| `check.log`, `bindings-check.log`, `e2e-perf.log` | Combined stdout/stderr, written as each stage runs |
| `stages.tsv` | Completed stage and exit code, one row per stage |
| `result.txt` | Overall exit code, written on exit; absent while running or after an untrappable interruption |

Pre-commit buffers terminal output until the hook finishes. During a long push,
inspect that run's `current-stage.txt` and tail its stage log. List evidence with
`rg --files --no-ignore target/qa/pre-push`; ordinary `rg --files` hides ignored
QA directories and logs. Concurrent invocations retain separate directories, but
do not run performance gates concurrently on the same machine.

These files are local, ignored evidence, not a durable handoff. Put the tested
commit, CI run link, result counts and remaining acceptance in the tracked landing
record before deleting temporary checkouts or ending the task. HEAD plus a status
listing does not fingerprint uncommitted contents, and a running check does not
prove a concurrently edited checkout. Keep the checked source unchanged.

`make verification-test` exercises the wrapper with real Git and Make in disposable
repositories. `tests/playwright-config.test.ts` checks actual Playwright collection
without starting browsers, covering the functional and production performance lanes.
