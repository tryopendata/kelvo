# Linux updater acceptance

These tests run real systemd services and real collector binaries in disposable,
privileged Docker containers. They do not mount the host's service directories,
publish a release, or connect to a real remote host. Containers are removed on exit.

Build the production agents with `make agent-linux`, then run from the repository:

```sh
python3 scripts/agent-update-qa/systemd.py
```

This covers both bundled architectures and both system/user installations: timer
configuration, disabled checks, deliberately stopped collection, invalid settings,
private-trial public access refusal, recovery of an interrupted SQLite migration,
postcommit history preservation, and uninstallation. To test one available native
architecture, pass `--binary src-tauri/binaries/agent/kelvo-agent-aarch64-unknown-linux-musl`.
Docker needs working amd64 emulation to execute x86_64 binaries on an arm64 machine;
a successful cross-build alone does not verify that runtime.

If the VM cannot retain a global emulator registration, both test scripts accept
`--emulator-dir /tmp/kelvo-qa-emulator`. This runs native arm64 systemd with the
actual x86_64 agent and registers its interpreter inside each disposable container.
Obtain the two static tools from the arm64 `tonistiigi/binfmt` image:

```sh
docker create --name kelvo-qa-binfmt tonistiigi/binfmt:latest
docker cp kelvo-qa-binfmt:/usr/bin /tmp/kelvo-qa-emulator
docker rm kelvo-qa-binfmt
```

The harness registers with `QEMU_PRESERVE_ARGV0=1`, so commands keep their original
arguments. It does not change the VM's global registration. This exercises the
x86_64 agent under emulation; it does not replace native x86_64 hardware acceptance.

## Signed release and process-death tests

Use `build_fixtures.py` to copy the source outside the repository, generate a
throwaway signing key, and build versions 0.2.0 and 0.2.1. It requires the pinned
Zig and cargo-zigbuild versions checked by `make agent-linux-tools`. Only the copy
receives the test public key and candidate fault behaviors. The production source,
key and endpoint remain unchanged; private key contents are never printed.

```sh
python3 scripts/agent-update-qa/build_fixtures.py \
  --directory /tmp/kelvo-signed-qa --target aarch64-unknown-linux-musl
python3 scripts/agent-update-qa/signed_release.py \
  --old /tmp/kelvo-signed-qa/aarch64-unknown-linux-musl-0.2.0 \
  --new /tmp/kelvo-signed-qa/aarch64-unknown-linux-musl-0.2.1 \
  --version 0.2.1 --test-key /tmp/kelvo-signed-qa/test-key --fault-fixtures
python3 scripts/agent-update-qa/build_fixtures.py \
  --directory /tmp/kelvo-signed-qa --target x86_64-unknown-linux-musl
python3 scripts/agent-update-qa/signed_release.py \
  --old /tmp/kelvo-signed-qa/x86_64-unknown-linux-musl-0.2.0 \
  --new /tmp/kelvo-signed-qa/x86_64-unknown-linux-musl-0.2.1 \
  --version 0.2.1 --test-key /tmp/kelvo-signed-qa/test-key --fault-fixtures
```

The failure fixtures attempt a forbidden filesystem write during private trial,
and return an error after becoming healthy and receiving SIGTERM. Both must roll
back. The test checks the real transient trial unit's sandbox and memory limits.
These behaviors are inserted only into the disposable source by the fixture builder.

The test creates a private CA and HTTPS server inside each container. Only that
container resolves `github.com` to its own server. Thus the real updater still
validates HTTPS certificates, release URL policy, manifest signatures, sizes and
digests. The fixture verifies check-only behavior and automatic installation,
kills the real updater before and after durable commit, verifies that a one-shot
manual request survives its caller, and checks executable permissions and host/
history identity. A signed manifest that relabels an older executable must roll
back and remain held until explicit retry or a newer release.

The transaction unit tests cover incomplete backups, surviving database writers,
repeated recovery, restored unit files, and loss of the updater between journal
cleanup and public startup. They use real temporary files, SQLite and OS locks.

## Recorded local acceptance

On 2026-10-25, both scripts passed for arm64 and x86_64 agents in system and user
installations under Debian bookworm systemd 252 in OrbStack. arm64 ran natively;
x86_64 ran through container-local QEMU with native arm64 systemd. The VM-wide
registration did not persist, so an amd64 systemd image could not run directly.
Native x86_64 hardware acceptance remains unverified.

The signed tests killed the updater in `Prepared` in the system layout and in
`Committed` in the user layout. All four combinations also rejected signed
relabeled versions, rolled back forbidden-write and post-health shutdown failures,
and verified the transient trial's actual sandbox and memory properties.

The user-layout uninstall test makes its unit directory read-only as uid 1000.
It verifies real permission errors remain in JSON stdout despite warning stderr,
while cleanup of the binary and history continues. The controller regression in
`crates/kelvo-agent/tests/support/update_reconnect.rs` uses an actual protocol
cursor before trial, rejects public access during trial, and reconnects with the
same cursor after rollback to verify new history remains reachable and failed
trial sequences were never exposed.
