# Remote Macs implementation

Status: implementation and automated verification complete, merged into main at a702cc4; real-device acceptance pending. Implementation commit: 6a914c0 (base a0eee11). The codex/remote-macs branch and worktree have been removed.

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
account signed into its desktop. Enter LAN and/or Tailscale aliases in Hosts → Add Mac.
Review unknown-host fingerprints against the other Mac before accepting them. Changed
keys and custom proxy trust configuration require native SSH resolution.

The flow reuses a compatible app or installs/updates the packaged app. An update requires
Kelvo to be quit on the other Mac and saves a matched app/data backup. macOS approvals
remain native and can be completed on the other Mac before retrying. Cancellation stops
read-only waits immediately, but waits for file swaps and settings writes to settle before
recovery. A completed fresh installation may remain for a later retry; no host row is
registered until handshake and local persistence succeed. Removal disconnects this
controller and optionally deletes only its local mirror.

The selected host supplies live metrics and history. Settings and process control remain
local. Unselected remote hosts send five-second summaries. Each mirror defaults to
100 MB (decimal); actual database/WAL size is checked after each committed page, with
pruning before the next page. Failed pruning is visible as history unavailable while
live metrics continue.

## Outstanding real-device gates

The user explicitly excluded testing another machine from the 2026-10-11 testing
follow-up. These checks remain future acceptance work, not a request for another Mac
or a blocker on that follow-up's automated coverage. Integration does not establish
these results. A future real-device pass would verify:

1. Signed app copy, Remote Login/key trust, native launch/login approval and resume.
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

## Single-Mac testing follow-up (2026-10-11)

Status: automated coverage and review complete on `codex/remote-qa` in
`/private/tmp/kelvo-remote-qa`; native QA blocked by computer-use tool access.
The branch/worktree remain available for that check before merge and cleanup.
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
  application heartbeat timeout, local history used for a remote host, and a removal
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
The execution skill's merge gate ("Nothing proceeds to merge until all checks pass")
is retained: native acceptance is still pending, so this follow-up is not marked fully
complete or merged. Another Mac is not required to finish this pass.

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
