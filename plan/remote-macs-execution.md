# Remote Macs implementation

Status (2026-10-12): Mac implementation, review fixes and SSH setup/error recovery
are merged and pushed to main through `ded22a7`. Automated verification passed;
real-device acceptance remains open. The initial implementation merged at `a702cc4`
(implementation `6a914c0`, base `a0eee11`); its branch and worktree were removed.
The latest SSH follow-up branch was also removed. See the dated records below for
the scope and evidence of each pass.

Execution follows ce:executing-plans (installed CE 4.3.0): isolated worktree,
subsystem agents, dependency gates, automated and real-device verification, mandatory
ce:code-reviewer review before integration. Preserve unrelated work in the original checkout.

## Approved design

One Kelvo app/collector/database per Mac. An owner-only control socket bootstraps
remote settings; a separate opt-in metrics socket serves local data through a bundled
stdio helper over persistent SSH. LAN and Tailscale aliases race with a 500 ms LAN
preference. No Kelvo backend. Host selection scopes the existing dashboard. Background
hosts use summaries; visible pages request live/process interest. Remote app Quit/logout
stops monitoring; hidden tray and closed windows do not.

Each remote gets an isolated SQLite mirror. Sync authoritative metric tiers, gaps and
events using stable source row IDs plus mutation sequences, durable page/cursor commits,
generation guards, bounded pages and separate pruning without synthetic roll-downs.
Preserve remote timestamps and use host clock context for relative queries. Historical
process/per-app attribution, process control, Linux and remote alerts are deferred.

Add Mac reuses or copies the packaged app over SSH, verifies supported OS/session and
identity, configures it through its settings owner, and resumes after any required OS
approval. Never bypass host-key or macOS trust. Existing settings/data survive failures.

## Task dependencies

- [x] A: Shared protocol and store contracts (storage agent); precedes B/C integration.
- [x] B: SSH/server/helper and isolated mirror sync (transport agent); depends on A.
- [x] C: Shell lifecycle, host registry/history routing/settings/IPC (orchestrator); depends on A/B.
- [x] D: Hosts UI, selection, setup, clocks, mocks (frontend agent); final integration depends on C.
- [x] E1: Automated verification, packaged build, browser visual checks and code review; depends on A-D.
- [x] SSH setup/error recovery follow-up: config-file detection, setup guidance, typed recovery, retry/trust state handling and regression tests; merged at `ded22a7`.
- [ ] E2: Real two-Mac acceptance and overhead measurements; deferred from the current testing pass by the user's explicit instruction not to test another machine.

## Verification gates

Packaged server launch and same-user sockets; LAN/Tailscale fallback and switching;
mutable gap/event replay; durable sync crash/reconnect/epoch handling; source clock skew;
local/mirror isolation; hidden-tray reopen and login; bounded slow-peer behavior; installer
cancellation/update rollback. Run Rust/frontend suites, bindings, make check and Playwright.
UI needs screenshot comparison. Real two-Mac LAN/Tailscale soak remains unverified and
is outside the current testing pass by user instruction. Packaged approval checks remain
open; do not substitute mocked results or mark them complete without evidence.

## Progress

- Worktree created at /private/tmp/kelvo-remote-macs; dependency installation complete.
- Protocol/store, transport and frontend agents completed their ownership groups.
- ce:code-reviewer approved the complete diff after the final cancellation fix.
- 909 frontend tests and the full Chromium/WebKit suite passed (236 passed, 8 existing
  skips), including all six remote flows. The initial full browser run had two failures;
  both passed isolated and the full quiet rerun passed. No root cause was established;
  the investigated Vite reload hypothesis did not reproduce. Screenshots compared with
  the design system, neighboring Overview screen and gallery in light/dark themes.
- The signed debug app was built with `APPLE_SIGNING_IDENTITY=- bun run build:app
  --debug --bundles app`; `codesign --verify --deep --strict` passes and its bundled
  helper reports version 0.1.0. Preserved artifact:
  `/private/tmp/kelvo-remote-build/Kelvo.app`.
- Transport tests exercise concurrent routes, source-clock skew, immediate multi-page
  catch-up, application heartbeat framing, exact fingerprint acceptance, and restoring
  both app and data after a simulated schema migration. The actual helper subprocess
  test verifies binary forwarding, flush, EOF cleanup and absence of a history database.
- Final `make check` passed: Rust formatting, workspace Clippy, App Store feature
  compile/Clippy, 755 Rust tests (28 existing ignored), Biome, TypeScript and
  909 frontend tests. Hardware tests ran outside the sandbox with approval; sandbox
  sysctl restrictions had caused the initial hardware identity test failure.
- Fifteen reviewer fault injections proved regression checks fail with the bug restored;
  all mutations were removed and restored tests passed. The final review verdict is APPROVE.
- `make bindings-check` and all commit hooks passed. Main integration preserved the
  concurrent Docker planning changes (D-103); remote monitoring is D-104. The final
  documentation review also approved the resolved merge and architecture summary.
- Merged to main at a702cc4. Removed the feature worktree, its dev server and branch;
  retained the signed app at the artifact path above. Unrelated work was preserved.

## Setup behavior

Use a packaged build from `bun run build:app`; ordinary cargo development does not
require a prebuilt helper. Both machines must be Apple Silicon Macs running macOS 26
or newer. Enable Remote Login and key-based SSH on the monitored Mac; use the SSH
account signed into its desktop. Enter LAN and/or Tailscale aliases, or `user@address`,
in Hosts → Add Mac. Both routes must reach the same Mac and account.
Review unknown-host fingerprints against the other Mac before accepting them. Changed
keys and custom proxy trust configuration require native SSH resolution.

The flow reuses a compatible app or installs/updates the packaged app. An update requires
Kelvo to be quit on the other Mac and saves a matched app/data backup. macOS approvals
remain native and can be completed on the other Mac before retrying. Cancellation stops
read-only waits immediately, but waits for file swaps and settings writes to settle before
recovery. A completed fresh installation may remain for a later retry; no host row is
registered until handshake and local persistence succeed. Setup enables remote
monitoring and launch at login on the other Mac, as disclosed in Add Mac.
Removal disconnects this controller and optionally deletes its local mirror.
"Also stop sharing on that Mac" is checked by default: it disables remote monitoring
for all controllers of that Mac, with a 15-second SSH timeout. Failure is reported
without preventing removal; launch at login is unchanged (D-105).

The selected host supplies live metrics and history. Settings and process control remain
local. Unselected remote hosts send five-second summaries. Each mirror defaults to
100 MB (decimal); actual database/WAL size is checked after each committed page, with
pruning before the next page. Failed pruning is visible as history unavailable while
live metrics continue.

### SSH setup and recovery

Add Mac and saved-host Settings check whether `~/.ssh/config` is a file on this Mac.
If missing, the guide opens automatically; direct `user@address` connections still
work without a config file. An unreadable path or other check failure is shown
separately, with details and **Recheck config**. Finding a file does not validate its
contents, discover aliases, or prove authentication. The setup guide does not create
or edit `~/.ssh/config` or authentication keys. Explicitly accepting an unknown host
does save its reviewed host key to the configured known-hosts file.

The guide explains Remote Login, the signed-in remote account, public-key creation
and authorization, SSH-agent unlocking, an optional config example, and LAN/Tailscale
addresses. Commands can be copied or selected manually if clipboard access fails.
Enter only an alias or `user@address`, not `ssh`, options or whitespace (255 characters
maximum). Nondefault ports and other SSH options belong in the SSH config entry.

For an unknown host in Add Mac, use **Verify host**, compare its fingerprint with
the other Mac, then accept it. Saved-host Settings provides a separate interactive
Terminal verification command (`BatchMode=no`, `StrictHostKeyChecking=ask`). Compare
the matching key type with the fingerprints shown by this command on the other Mac:

```sh
for key in /etc/ssh/ssh_host_*_key.pub; do ssh-keygen -lf "$key"; done
```

The guide also provides a strict access test (`BatchMode=yes`,
`StrictHostKeyChecking=yes`, `ControlPath=none`). It must succeed without a password,
passphrase or trust prompt; success exits without output. Interactive verification
alone does not establish that Kelvo can connect. Save any address edits, then use
**Retry** in Hosts. A changed key requires confirming why it changed and resolving
trust in Terminal; the app never replaces a known key automatically.

| Failure | Recovery shown in setup/settings |
| --- | --- |
| SSH authentication | Check the signed-in account and Remote Login access, authorize a key, and unlock it in the SSH agent |
| Host key needs verification | Compare fingerprints; investigate changed keys before resolving trust |
| Unsupported Mac or Kelvo version | Check Apple Silicon/macOS 26 requirements and the diagnostic for the version mismatch |
| Different Mac answers | Correct the alias/address/account, or add a replacement as a new Mac |
| Connection/setup failure | Keep the actual diagnostic visible: this also covers installation, protocol and local I/O failures, not just network reachability |

Other typed errors retain their diagnostic. Address edits clear obsolete errors and
trust prompts; expired trust tokens return to verification. Failed operations keep
form inputs, and unexpected failures are logged with the operation and destination.

## Outstanding real-device gates

The user explicitly excluded testing another machine from the 2026-10-11 testing
follow-up. These checks remain future acceptance work, not a request for another Mac
or a blocker on that follow-up's automated coverage. Integration does not establish
these results. A future real-device pass would verify:

1. Signed app copy, Remote Login/key trust, native launch/login approval and resume.
   Exercise the setup guide with no config file, direct `user@address`, a passphrase
   key/SSH agent, unknown and changed host keys, and saved-host address edits.
2. Live CPU/memory/process values against the source Mac, including held samples and
   positive/negative source clock changes.
3. LAN preference, Tailscale fallback, return to LAN after network change, sleep/wake,
   authentication/identity failure and application-stall error states.
4. Offline mirrored history, incremental catch-up, source epoch reset, history failure,
   per-host cap enforcement and cancellation/update recovery.
5. Cold Finder launch and warm reopen with hidden tray, login without windows, and Quit.
6. Measured two-machine CPU/memory/bandwidth, slow-link live latency during catch-up,
   and multiple controllers/demand release. No hardware overhead claim is made yet.

SSH alias suggestions remain part of the broader v4 roadmap. This milestone accepts
explicit aliases and follows their existing SSH configuration.

## SSH setup and review follow-up (2026-10-12)

- [x] Pulled the remote-settings deadlock fix through `1b2e535`. The fix's real-device
  evidence covers control responses, an off/on sharing toggle and metrics-socket
  creation; an end-to-end two-Mac connection is still unverified. See the
  [deadlock record](PROGRESS.md#2026-10-12-remote-access-settings-deadlock).
- [x] Implemented the setup detection and recovery behavior above. `ce:code-reviewer`
  found two blockers: saved-host guidance needed interactive trust verification,
  and the broad connection-error kind incorrectly implied a network problem.
  Both were fixed, their regressions failed before the fixes, and re-review approved.
- [x] Final `make check`: 965 frontend tests and 839 Rust tests passed (32 ignored),
  plus formatting, lint, typecheck and standard/App Store Rust checks. Full browser
  suite: 286 passed, 10 existing skips. Light/dark Chromium/WebKit screenshots were
  compared with Hosts, Settings and the gallery; visual differences are recorded in
  [the progress log](PROGRESS.md#2026-10-12-ssh-setup-guidance-and-error-recovery).
- [x] Committed, merged and pushed at `ded22a7`; commit hooks and the push gate
  (`make check bindings-check e2e-perf`) passed. Removed `codex/ssh-setup-recovery`;
  unrelated local planning files were preserved.
- [ ] Native two-Mac setup and actual SSH-agent interoperability. Browser mocks and
  the passing performance gate do not establish these or remote-host overhead.

## Single-Mac testing follow-up (2026-10-11)

Status: automated coverage and review complete; merged into main at `9e69367`.
The user explicitly authorized merge, cleanup and push based on the passing tests.
Native QA remains unverified because computer-use tool access was blocked.
The user requested ce:writing-tests and packaged-app manual QA, then explicitly excluded
testing against another machine. This pass prioritizes real protocol/socket/SQLite
integration, user-visible failure/retry/cancellation/removal flows, and a packaged app
with an isolated QA identifier. Two-machine acceptance is excluded from this pass, not
claimed as verified. Results and discovered fixes are recorded below.

- [x] Seven real server/session integration tests: independent controller demand,
  fragmented incoming frames during outgoing traffic, a stalled viewer alongside a
  healthy viewer, durable SQLite resume/replay, shutdown/restart, oversized-frame
  rejection with listener survival, and refusal to overwrite an ordinary socket-path file.
- [x] Application heartbeat timeout with a real framed peer and virtual time. The peer
  stays connected but stops answering; the client times out and closes its stream.
  The source now uses Tokio's monotonic `Instant` so this deadline can be tested without
  a 30-second wall-clock wait. Tokio test utilities are a dev dependency.
- [x] Four frontend workflow tests: failed setup/retry, offline reconnect/retry, failed
  removal/retry, and remote/local/offline history isolation. Cancellation additionally
  proves no host is registered and a reopened setup form resets. These use the real
  app/providers/hooks with the repository's IPC boundary mock.
- [x] Chromium/WebKit host lifecycle: add, rename, disable, select, remove, return to local
  metrics. The scoped remote suite passed 8/8; fourteen screenshots were captured.
- [x] Seven fault injections made the new assertions fail as intended: leaked process
  subscriptions, cancellation-unsafe framing, disabled slow-write timeout, disabled
  application heartbeat timeout, local history used for a remote host, a removal
  error hidden by closing its dialog, and an entrance replay during live ticks. All
  injected faults were restored.
- [x] `make check`: 763 Rust tests passed (29 ignored, including the new opt-in native
  test), 913 frontend tests in 142 files, Rust formatting/Clippy, App Store checks,
  Biome and TypeScript passed.
- [x] Full browser rerun: 238 passed, 8 existing skips across Chromium and WebKit after
  correcting a diagnosed pre-existing motion-test race.
  The first run had 237 passes, 8 skips and one failure: the live-tick test waited for
  all animations to stop, counting 22 intentional gauge transitions. Diagnostics in
  both engines found those transitions and zero entrance animations. Its settling
  precondition now targets entrances; frozen-tick all-motion checks remain unchanged.
  The motion suite passed 10/10, the injected replay failed as expected, and the restored
  live-tick check passed in both engines. TypeScript and Biome passed after this correction.
- [x] `make bindings-check` passed; generated bindings are unchanged.
- [x] Built an ad-hoc signed isolated app (`com.tryopendata.kelvo.remoteqa`), verified
  its signature and executed its bundled helper's version command. Artifact:
  `/private/tmp/kelvo-remote-qa-artifacts/Kelvo Remote QA.app`.
- [ ] Native manual QA and the opt-in `packaged_app` acceptance test. The app has not
  been launched: computer-use app selection repeatedly returned `Computer Use was not
  approved to use Kelvo Remote QA`, including after the user granted consent and showed
  Any App enabled. This is a tool-access blocker, not missing user consent. No other
  UI automation path was used to bypass it. The everyday installed app was untouched.
- [x] ce:code-reviewer review and fix iteration: the native test's restoration
  guard now validates the complete settings response and disarms after a confirmed
  restore. Both that review and the final motion-test/docs delta review approved with
  no remaining findings.

Logs and browser artifacts are preserved under `/private/tmp/kelvo-remote-qa-artifacts/`.
The user's explicit integration instruction supersedes the earlier hold for native QA.
All executed checks pass; integration does not mark the unrun native checks as verified.
Another Mac is not required for those checks. The `codex/remote-qa` branch and its
worktree have been removed; the isolated app and verification artifacts remain at the
artifact path above. Unrelated worktrees and untracked planning assets were preserved.

The native acceptance test is compiled but deliberately ignored by the default suite.
After launching the isolated QA app, run:

```sh
KELVO_QA_DATA_DIR="/Users/riley/Library/Application Support/com.tryopendata.kelvo.remoteqa" cargo test -p kelvo-remote --test packaged_app -- --ignored --nocapture
```

It checks actual control/metrics sockets, advancing collector samples, memory values,
process rows, heartbeat, access revocation, and restoration of the prior remote-monitoring
setting. This does not require another Mac. Native onboarding/Hosts/error/cancel and
hidden-tray close/reopen/Quit checks remain pending alongside it. No new hardware
accuracy, SSH loopback, two-machine, or overhead result is claimed.
