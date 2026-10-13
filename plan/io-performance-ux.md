# I/O performance and process attribution

Status: Implemented and installed, 2026-10-13. Counter validation, native workflows, responsive/theme checks, automated suites and CE review passed. Matched native whole-app CPU measurement remains unverified because the benchmark requires an active display; see [verification evidence](PROGRESS.md#2026-10-13-disk-io-implementation).

Extend **Disk** to answer “Is storage slowing this Mac down, and which process was doing the work?” Keep network traffic in **Network**, and make **Processes** the place to inspect one process across resources. CPU remains focused on compute. This follows the resource organization already used by Kelvo and avoids putting disk activity behind a CPU label.

## Problem and outcome

When a build, import, database, or desktop application feels slow, the user wants to distinguish CPU work from waiting for data. Today Kelvo's Disk page already shows live read/write throughput, history and byte totals, and process attribution for a selected interval. Processes already has read/write rate columns. These show activity, but do not establish whether storage is the bottleneck. The attribution table also sits below volume capacity, interrupting the diagnostic flow.

The working hypothesis is that a shared time window for storage response time, workload, and process attribution will let the user investigate an I/O slowdown without switching tools. The evidence is the user's report and the existing data paths; task-based acceptance below will test the hypothesis.

High MB/s is not proof of saturation. Small random requests can create long waits at modest throughput, and a fast SSD can sustain heavy sequential transfers comfortably. IOPS counts operations; response time measures how long completed requests took. Memory pressure and swapping can also create storage work, so the experience needs a link to Memory when that context is relevant.

## Existing conventions

| Product | Pattern | Implication for Kelvo |
| --- | --- | --- |
| [Apple Activity Monitor](https://support.apple.com/en-ca/guide/activity-monitor/actmntr1005/mac) | Disk separates transfer amounts from accesses and charts reads/writes. | Keep storage under Disk and label units explicitly. |
| [iStat Menus](https://bjango.com/mac/istatmenus/) | Disk usage/activity and network monitoring have separate destinations. | Preserve familiar resource navigation. |
| [Netdata](https://www.netdata.cloud/blog/monitoring-disks-understanding-workload-performance-utilisation-saturation-latency/) | Storage analysis combines workload, operation time and saturation signals. | Put responsiveness beside workload; a single throughput graph cannot diagnose a bottleneck. |

The [USE method](https://www.brendangregg.com/usemethod.html) distinguishes utilization, saturation and errors. Adopt that distinction without inventing a universal “I/O %” gauge. Neither a chart reaching its automatically scaled ceiling nor summed concurrent request durations represents a drive's maximum capacity.

## Proposed Disk experience

Use the existing SF typography, Disk blue accent, chart surfaces, range picker and brush interactions. Follow `ce:design`'s data and analysis direction with the repository's density and token rules taking precedence. No new sidebar destination or status-card grid.

Order the page around diagnosis:

1. **Disk I/O** chart, read and write in MB/s, retaining current values and historical range totals. A device selector defaults to the internal physical drive; “All devices” remains available for throughput. Display the actual device name so external storage is discoverable.
2. **Average request time** directly beneath it, read and write in milliseconds, using the same time axis and crosshair. Show **Operations/s** as the alternate view in this region. Keep average request time visible by default because the user's question is about waiting. Each point describes completed driver requests, not application response time.
3. **Processes** table immediately below the charts. In Live, default to Read/s, Write/s and Total/s, sorted by Total/s. In a selected historical interval, show Read, Written, Total and average rate over the covered duration, with the selected interval in the heading. Clicking a process opens its existing detail while preserving the time selection.
4. **Volumes** and capacity after attribution. Capacity remains useful but does not interrupt the path from slow storage to the responsible workload.

For “All devices,” response times need operation-weighted aggregation and a visible warning that a slow device can be hidden by a busy fast one; prefer individual-device comparison if the backend cannot provide a meaningful aggregate. Process accounting is currently host-wide: a device-filtered chart must label its table **Processes on this Mac**, rather than imply that each process's bytes belong to the selected drive. Do not add process byte counts to device byte counts; filesystem work, caches and attribution boundaries differ.

Brushing any chart selects the same interval for all charts and the process table. Historical investigation must retain exited processes and distinguish PID reuse using the existing process identity. Live resumes explicitly. Null means unavailable, an unmeasured span remains a gap, and measured zero remains zero. If no operations completed, average request time is “— · No completed operations,” not 0 ms.

## What can establish a bottleneck

The first useful increment is workload plus response-time history and process attribution. Label it **Disk I/O**, without a categorical “maxed out” indicator. A later “Storage contention” status requires a validated signal of queued work or sustained latency degradation; its cause and duration must be inspectable. A high response time can also reflect device errors, power management or an external link, so “busy” and “slow” are separate facts.

This Mac's read-only IOKit inspection found nonzero operation and total-time counters for the internal SSD. Apple's [IOBlockStorageDriver source](https://github.com/apple-oss-distributions/IOStorageFamily/blob/main/IOBlockStorageDriver.cpp) adds elapsed nanoseconds per completed request; average request time can be derived from changes in total time divided by completed operations. The same implementation passes zero for its separate latency-time argument. Those counters cannot supply queueing delay here. Driver timings can overlap: dividing their sum by wall time must not be presented as a bounded utilization percentage.

Before committing the metric contract, validate the units and semantics on this Mac against controlled sequential and random I/O, an idle interval, and a reset/reconnect. Use a disposable bounded file, not a destructive device benchmark. Confirm externally attached drives and drivers that omit counters. Keep unsupported measurements unavailable instead of estimating them from throughput. This collection spike is the next technical task.

## Scope and acceptance

The initial scope is macOS storage I/O, current and historical process attribution, and navigation between Disk and Processes. Network remains separate. Per-process I/O wait time, precise per-device process attribution, queue depth, automatic alerts, universal saturation percentages and a privileged tracing service are deferred until their collection and overhead are demonstrated.

Acceptance tasks:

- Identify the process producing the most disk traffic within 10 seconds in Live.
- Select an earlier slowdown and identify the busiest retained process within 30 seconds.
- Distinguish high throughput with short requests from modest throughput with slow requests in labeled fixtures.
- Read the selected device, interval, units and missing-data state without opening settings.
- Keep charts and the first five attribution rows visible at 1280×900, support 360 px without page overflow, and pass keyboard, contrast and reduced-motion checks.
- Validate new counters against real hardware and meet the existing collection, storage and UI budgets without raising thresholds.

## Implementation sequence

1. Validate the counter source with bounded uncached file I/O and a disposable device reconnect. Completed on the development Mac; see [progress evidence](PROGRESS.md#2026-10-13-disk-io-implementation).
2. Collect independent operation rates and paired completed-operation/time deltas in Rust. Preserve paired integrals through open buckets and tiered rollups ([D-110](decisions.md#d-110-disk-request-history-preserves-completed-operation-integrals)); publish a bounded device/history query and live views through generated bindings.
3. Build the Disk device selector, throughput and request-time/operations views, shared crosshair/brush, and host-wide process attribution. Keep an unavailable selected device stable while investigating history.
4. Verify real collection, reset/missing data, unequal operation weights, PID reuse, responsive layouts, native UI behavior, and existing performance gates. Review the integrated change before merging.

## Handoff

Technical planning should first validate the operation/time counters, then specify Rust-owned deltas, reset behavior, operation-weighted history and coverage. Preserve the series model and explicit gaps in `plan/architecture.md`. Reuse the existing process read/write collection and attribution queries; do not introduce a second sampler or per-process query fan-out.

Design should extend the existing Disk page and process table. The core interaction is one shared time selection leading from response time to workload attribution. The unresolved product condition is whether macOS exposes sufficient evidence for a trustworthy contention status; the proposal remains useful without claiming that it does.
