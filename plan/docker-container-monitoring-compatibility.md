# Docker monitoring compatibility evidence

**Observed:** 2026-10-11, approximately 06:19–06:25 UTC, macOS 27.0.1 (26A434),
Apple Silicon. **Work item A: partial; Desktop attribution and Resource Saver
verification are blocked by the available environment.** This evidence does not
complete A, qualify a supported release, or satisfy H1.

## Installed-runtime matrix

| Runtime | Local evidence | API result | macOS attribution | Idle behavior |
| --- | --- | --- | --- | --- |
| Docker Desktop | No app in `/Applications` or `~/Applications`; `~/.docker/run/docker.sock` absent; no Desktop context or matching runtime process observed | Not exercised | Not exercised; required release gate remains open | Resource Saver not exercised |
| OrbStack 2.2.3 (build 20963) | App installed; selected context `orbstack`; local Unix socket reachable | Engine 29.4.0, server API 1.40–1.54; explicit downgrade to 1.47 accepted for info and inventory | Socket-owning Helper executable and PID/start identity verified; Docker-only scope unproved | Not exercised; two containers were running |
| Colima | No binary in `/opt/homebrew/bin` or `/usr/local/bin`; no `~/.colima/*/docker.sock`; no Colima context or matching runtime process observed | Not exercised | Not exercised | Not exercised |

Absence is limited to these discovery locations and current processes, not proof
that software is nowhere on the disk. No software was installed or launched and
no runtime/container lifecycle operation was performed.

## Discovery and API observations

Read only `currentContext` from Docker's config and local Docker endpoint/name
fields from context metadata. No CLI was invoked and no credentials were printed.
The selected context points to `unix://~/.orbstack/run/docker.sock` (with an absolute
home path in the actual metadata). `/var/run/docker.sock` resolves to that same
socket, so these are one candidate, not two engines. The socket is a Unix socket
with mode 0755. Desktop's default socket is absent.

A direct Unix HTTP connection issued `GET /version`, intersected the server range
with the probe's explicit 1.40–1.47 range, and selected 1.47. Both
`GET /v1.47/info` and `GET /v1.47/containers/json?all=1` returned HTTP 200. The
ceiling deliberately exercises downgrade negotiation; it is **not** a pinned
Bollard version or a product API-support decision. Bollard negotiation still needs
its own real-engine test when the adapter is implemented.

Allowlisted observations:

| Fact | Value |
| --- | --- |
| Server OS / architecture | Linux / arm64 (`info`: aarch64) |
| Cgroup version | 2 |
| Engine vCPUs | 16 |
| Engine memory capacity | 16,818,663,424 bytes |
| Inventory | 8 containers: 2 running, 6 exited |
| Daemon identity | Present; raw value deliberately omitted |
| Probe request sequence elapsed | 90.1 ms on one run; not a performance benchmark |

Inventory does not establish stat semantics, metric accuracy, event/log support,
capacity changes, cancellation, permission failures, incompatible live servers,
or supported container population size. Inspection/stats/events/logs were not
requested. Container names, IDs, labels, commands and environment were not saved.
The inventory endpoint includes fields outside the monitoring allowlist; the
bounded response was decoded in memory, summarized, and discarded without a raw
payload dump.

## Runtime-process attribution

`ps` executable candidates were independently checked using `proc_pidpath`;
`proc_pid_rusage(RUSAGE_INFO_V0)` provided physical footprint and start identity.
A second rusage read about 100 ms later confirmed an unchanged start identity.
`lsof -n -a -U <socket>` linked the actual Unix socket to Helper PID 1462.
No process arguments or environments were inspected.

| PID | Verified executable within `/Applications/OrbStack.app` | Parent | Start (Mach absolute units) | Physical footprint at observation |
| --- | --- | --- | --- | --- |
| 1439 | `Contents/MacOS/OrbStack` | launchd (PID 1) | 747586077 | 92,718,064 bytes |
| 1462 | `Contents/Frameworks/OrbStack Helper.app/Contents/MacOS/OrbStack Helper` | launchd (PID 1) | 762328221 | 1,171,872,752 bytes |

These are ephemeral measurement identities, not persisted product identifiers.
They establish access to these processes and the Helper's socket ownership, not
that the two processes exhaust the runtime, belong to a verified coalition, or
that all Helper memory serves Docker. Both are launchd children; ancestry alone
does not connect them. Code-signing identity and profile/VM ownership were not
verified. No CPU-rate or energy comparison was made.

For this observed OrbStack combination, the future UI can safely report attribution
as unavailable until its mapping is implemented, or describe verified processes
as **shared OrbStack runtime** with the explicit limits above. It cannot label
their footprint as Docker-only, add it to container memory, or infer overhead.
Desktop's required real attribution check remains blocked.

## Idle and Resource Saver

There is no sleep-safety result. OrbStack had running workloads; stopping them
would disturb the user's environment. Desktop was unavailable. An ordinary
successful read is not proof that a probe preserves sleep.

[Docker's Resource Saver documentation](https://docs.docker.com/desktop/use-desktop/resource-saver/)
describes stopping the Linux VM after an idle interval (five minutes by default)
and notes that some reads can be served without waking it. This does not establish
that this monitor's specific requests or held streams are harmless.

Before claiming sleep-safe monitoring, use an available Desktop installation with
no running containers and record its version, idle timeout, and VM/process state:

1. Establish Resource Saver entry with the monitor disabled.
2. Compare the proposed cadence and individual request/stream types against that
   baseline through an entire idle interval; record VM wake and host/runtime CPU.
3. Repeat after cancelling stats and events at idle, including any proposed
   low-frequency discovery request.
4. If a discovery request wakes or prevents sleep, use full idle suspension and
   explicit Resume/check, with an unobserved coverage interval.

Until tested, do not implement an automatic idle polling claim based on this
spike. OrbStack/Colima need their own idle observations as available.

## Reproduction and probe verification

From the worktree root:

```sh
python3 scripts/docker-compatibility-probe.py ~/.orbstack/run/docker.sock
python3 scripts/docker-compatibility-probe.py ~/.docker/run/docker.sock
```

The first returned `connected`; the second returned `unavailable` with
`FileNotFoundError` and exit status 1. The script requires an explicit local
socket; it cannot send arbitrary endpoints, lifecycle methods, TCP or SSH
requests. It makes only version/info/inventory GETs, caps each response at 2 MiB
and inventory at 10,000 entries, uses two-second socket timeouts and a five-second
total deadline, and prints only numeric facts, canonical states and API versions.
It is a one-shot developer probe, not production discovery or a polling worker.
Calling it on an idle runtime may wake it.

Additional local fake-Unix-server checks passed: oversized response rejection,
five-second total deadline despite slowly arriving header bytes, rejection of
non-allowlisted targets before connecting, and malformed API-version rejection.
These ad hoc checks supplement the real requests; they are not a product test
suite. No feature source code, schema, transport or release support was added.

## Implementation follow-up, 2026-10-11

The pinned Bollard 0.19.4 adapter subsequently negotiated API 1.49 with this
OrbStack installation. Its ignored, explicitly invoked read-only integration test
passed inventory, inspection and stats, including CPU, charged memory and inactive
file-cache fields. The independent worker normalized both running containers
within 1.12 seconds and cancelled after monitoring was disabled. These checks do
not establish aligned ±5% accuracy against `docker stats`, population performance
at 32/128 containers, or compatibility with absent runtimes.

Fake Unix-engine tests additionally exercised the eight-request concurrency bound,
fair scheduling across 40 delayed containers, stale-generation cancellation,
quiet shared log streams, the two-stream admission limit and recovery, TTY and
multiplexed tails, and isolation of a 3 MiB log burst from metrics/events. Transport
tests verified the GET allowlist, raw response cap and quiet event streams lasting
beyond the finite request deadline. Synthetic protocol evidence supplements the
installed-runtime observations; it does not replace them.

The product currently reports runtime-process attribution as unavailable, with
shared OrbStack scope explained, and fully suspends collection after observing an
idle engine until explicit resume. Neither unavailable attribution nor that
conservative fallback is presented as a measured Resource Saver result.

### Short diagnostic and hardware measurement limits

A later read-only oracle bracketed four Docker CLI stats reads with Engine API
stats reads for two existing running containers (eight paired observations).
Across 2.005–2.074-second brackets, the API working-set formula differed from the
CLI by at most 0.0129% outside the observed before/after range; effective limits
differed by at most 0.023%, consistent with CLI human-unit rounding. Numeric-only
output is in `/tmp/docker-memory-oracle.log`. This checks the Engine API formula
against the CLI, not sampled product-worker output. CPU windows were unaligned;
end-to-end ±5% accuracy and 32-container validation remain unverified.

After the final backend review fixes, both explicitly invoked read-only installed
runtime tests passed again: API 1.49 negotiation, inventory, inspect and stats
(0.16 seconds), then worker normalization of both running containers and disable
cancellation (1.05 seconds). These checks used the final inspection-timestamp and
catalog code. No runtime or container lifecycle operation was performed.

A six-minute release-mode headless background diagnostic kept both running
containers measured. It used an isolated temporary history database, averaged
0.0758% process CPU, reached 17,268,736 bytes peak RSS, and observed at most
6.44 ms history-query latency. Pending writes reached 30 before the five-minute
commit and returned to 1 afterward; shutdown flushed successfully. A separate
60-second module-off diagnostic observed 0.003–0.010% process CPU. These runs
preceded the final review fixes and exclude WebKit and daemon/VM cost. They do
not establish the whole-app overhead budget, population-scale behavior or a
24-hour soak.

The isolated main-baseline app was built at `4c44aae`, but the tray benchmark
refused to run because the Mac lid was closed with no external display and
lid-close sleep enabled. No whole-app comparison was obtained. The harness
guard remains intact; a valid hardware run and the integrated soak are still
required unless the user explicitly defers those acceptance items.

## References checked during the spike

- [Docker Engine API negotiation](https://docs.docker.com/reference/api/engine/):
  select a mutually supported version and gate capabilities when downgrading.
- [OrbStack Docker context and socket](https://docs.orbstack.dev/docker/#docker-context-socket):
  documents its context and optional `/var/run/docker.sock` alias; local findings
  match this installation.
- [Colima FAQ](https://colima.run/docs/faq/): discovery starting point only; no
  installed Colima combination was exercised.
- Local macOS SDK `<sys/resource.h>` and `<sys/proc_info.h>`, and Kelvo's
  `crates/kelvo-collect/src/macos/libproc.rs`, supplied the process-measurement
  structure definitions and existing collector conventions.
