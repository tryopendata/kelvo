# Automatic updates

Status: implemented and code-reviewed on `codex/automatic-updates`. A real macOS
login-session smoke check remains before merge/publication. This is not a release claim.

## Tasks and dependencies

- [x] Authenticated release catalog and standalone Linux assets (foundation).
- [x] Linux scheduled installation, isolated trial and crash recovery (depends on catalog).
- [x] Mac idle installation, durable staging and noninteractive authorization policy (depends on catalog).
- [x] Host-owned controls, protocol compatibility and UI (depends on platform interfaces).
- [x] Full checks, signed package verification, native restart harness and code review.
- [ ] Fresh-account/login-session rollout acceptance before merge/publication.

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
the documented coordinated external reinstall. Keep this branch unmerged until the
remaining login-session acceptance is completed; no release was created or published.
