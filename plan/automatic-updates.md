# Automatic updates

Status: landed on main and verified. The user authorized landing with the
real macOS login-session check outstanding. That check remains release acceptance;
this is not a release claim.

## Tasks and dependencies

- [x] Authenticated release catalog and standalone Linux assets (foundation).
- [x] Linux scheduled installation, isolated trial and crash recovery (depends on catalog).
- [x] Mac idle installation, durable staging and noninteractive authorization policy (depends on catalog).
- [x] Host-owned controls, protocol compatibility and UI (depends on platform interfaces).
- [x] Full checks, signed package verification, native restart harness and code review.
- [ ] Fresh-account/login-session rollout acceptance before publication.

## Agreed behavior

Automatic installation defaults on, with existing automatic-check opt-outs preserved.
Each remote updates independently. Macs wait for interactive windows and maintenance;
ambient desktop boards do not block, board editing does. Automatic releases must retain
the installed protocol range. Existing installations need one initial manual upgrade.

The signed catalog authenticates version, protocol range and platform artifact digest,
size and URL. Tauri still verifies the Mac archive and signed version. Linux performs
the installation under a host-owned lock, independently of SSH, and preserves identity,
settings, history and service scope.

## Review requirements

The CE devil's-advocate review found two blockers, resolved in the design:

- A trial Linux daemon must not expose history before durable commit. Rollback restores
  database sequence counters, so exposing trial rows would invalidate controller cursors.
  After commit, recovery must never restore the pre-update database.
- Tauri's permission-error fallback opens an administrator prompt. The pinned updater
  patch must disable elevation for automatic calls while retaining manual authorization.

Check interruption before commit, after commit but before public serving, and during
recovery. Test actual signatures, files, SQLite and service processes. Native Mac restart
and Linux systemd/reboot evidence are separate from browser/mock tests.

## Verification

Final `make check` passed: 1,109 workspace Rust tests, 26 vendored-updater tests,
and 1,289 frontend tests in 199 files. The workspace retained 48 existing ignored tests.
Formatting, clippy, App Store compilation, architecture fixtures, lint and typecheck
passed. Generated bindings were regenerated. Chromium and WebKit passed all 436
functional tests; the 13 frontend performance cases passed without changing budgets.

Shared verification tests use real signatures, files and local HTTP downloads. The
signed release catalog and all three advertised artifacts passed verification with the
production public key. The Mac archive rejected tampering without changing the old
bundle, installed into a temporary bundle, and passed strict code-signature verification.
Its bundled Linux agents matched the standalone signed catalog. App/helper dependencies
were all supplied by macOS. Nothing was published or installed over the user's app.

Linux signed HTTPS installation and real systemd lifecycle tests passed for both musl
architectures in system and user layouts. They covered opt-out, invalid settings,
stopped collectors, sandbox violations, failed shutdown, private trial history, rollback,
pre/postcommit process death, identity/history retention and truthful uninstall failures.
The x86 binary ran through container-local QEMU under native arm64 systemd; native x86
hardware was not tested. Temporary containers and emulator registration were removed.
See [reproduction details](../scripts/agent-update-qa/README.md).

A real controller reconnects with its original cursor after a failed private trial and
never receives trial sequences. Regression mutations failed when signature validation,
protocol containment/equality, the public-serving gate, uninstall reporting, Mac cache/
installation gates, permission restrictions or upgrade-guide access were removed.

Native Tauri restart acceptance passed ordinary, background and remote-setup launches:
real shutdown, preserved host identity/history, a new process, and no restarted windows.
The Settings and Hosts screenshots match existing row controls, typography and spacing
in the design system and gallery. The host dialog is taller to accommodate per-host
policy/results; incompatible releases add an actionable upgrade-guide button.

The CE code reviewer approved after all six findings were fixed: trial sandbox parity,
clean trial shutdown, truthful uninstall reports, signed/actual protocol equality,
actionable coordinated-upgrade guidance, and the real reconnecting-controller regression.

Earlier runs exposed a stale unit-text expectation and transient browser failures; the
expectation was corrected and the frozen-source full browser run passed. Initial engine
allocation failures during concurrent builds passed sequential baseline/candidate checks
and the final full gate. No performance thresholds were changed.

## Remaining rollout acceptance

A genuine macOS login-session launch and fresh-account update remain unverified. Archive
installation and native restart were exercised separately; this does not claim a full
previous-release-to-new-release update through an actual login session. Native desktop
boards do not yet exist, so ambient-board/editing behavior cannot be exercised in this
version. Future board editors must participate in the existing maintenance gate.

Existing installations need one initial manual upgrade. Incompatible releases require
the documented coordinated external reinstall. The user subsequently authorized pushing
to main despite the remaining login-session acceptance; no release is authorized by
that instruction.

## Landing

Implementation `c434a4e18d27b561fa7309cbd5eaf29dcac7d18f` was reviewed and integrated
with main's release documentation. Source landed on main as
`6931c0843e86ed010655792260705c051edeecd0` through the normal push hooks.
[Exact-source CI](https://github.com/tryopendata/kelvo/actions/runs/40266417306)
passed all five jobs, including Linux, frontend and both advisory audits. Frontend
unit tests passed 1,289 cases in 99.38 seconds. Browser checks passed 218 Chromium
cases in 6.3 minutes and two WebKit shards of 109 cases in 4.4 and 4.7 minutes,
respectively: 436 total, with no retries or flaky results.

The local push ran `make check bindings-check e2e-perf`: 1,109 workspace Rust tests,
26 updater-patch tests, 1,289 frontend tests, and 13 performance cases passed. The
workspace retained 48 ignored tests. Performance cases took 5.0 minutes, recorded
no long tasks in the measured product scenarios, and retained all existing budgets.

Hosted Linux passed 877 portable test executions (26 ignored), 11 native x86 musl
pipe tests, and both release history-fill/read tests. Both static architectures passed
ELF alignment checks. The 150-series history used 90.2 MB against the 150 MB budget;
30-day reads completed in 93.5 ms, heatmap in 9.7 ms and CSV export in 117.5 ms.
Frontend audit found no vulnerabilities across 271 packages. Rust advisories and
bans/licenses/sources checks passed.

The first push exposed an existing fixture isolation bug: inherited Git hook variables
made a temporary-repository test target its caller. Restored the caller's bare-mode flag,
verified both checkout states, and isolated fixture Git commands from `GIT_*` variables.
All nine hook tests passed; removing the isolation in a disposable copy made the new
regression fail by staging into its caller. The successful push reran every local gate.

Local main was advanced while preserving the unrelated UX planning changes. Acceptance
logs, screenshots, temporary-package evidence and a recovery copy of those planning edits
were retained under `target/qa/automatic-updates-6931c08/` in the primary checkout.
These results describe source revision `6931c08`; the subsequent documentation-only
landing record does not claim a separate source-verification run.
