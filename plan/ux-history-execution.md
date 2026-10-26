# Durable usage execution evidence

Status: feasibility prototype and typed contracts; production integration has not passed its gate.

The schema contracts live in `crates/kelvo-schema/src/durable_usage.rs`. They separate
requested bounds from retained observation spans and preserve per-resource support,
measured duration, source incompleteness, nullable values and uncertain-time network
bytes. Process expansion identifies instances by PID and start time and exposes no
current process actions. Explicit app membership supports comparison without ranking
the previous interval independently. Existing live-ring usage contracts are unchanged.

## Real current-store baseline

2026-10-25, native macOS collectors, 198 series. Command:

```sh
cargo test -p kelvo-engine --test perf_gates store_write_volume_per_hour_at_one_second -- --exact --nocapture
```

One test passed. Ten minutes of fake sampler time, actual native collectors and SQLite;
flushes simulate the existing five-minute commit interval. Reported hourly values:

| Current table | Rows/hour | Payload bytes/hour |
| --- | ---: | ---: |
| tier_10s | 360 | 552960 |
| tier_1m | 60 | 92160 |
| proc_snap | 114 | 95760 |
| proc_net_10s | 342 | 23472 |
| proc_net_1m | 60 | 5040 |
| Other enumerated gate tables | 0 | 0 |
| Total | 936 | 769392 |

WAL: 4,331,712 bytes/hour, twelve commits/hour. The baseline uses the existing gate's
table enumeration; it is not a claim that all database index and identity costs are
payload. WAL includes actual SQLite writes. This was not an idle CPU or latency test.
A separate executable-path grouping counted 1,087 current-user processes and 425
app/name identities on this machine; this is a cardinality proxy, not collector output.

## Candidate write measurements

`python3 scripts/bench-usage-history.py -h` documents the reproducible probe. It creates
and deletes temporary SQLite files, never opening the user's history. Packed LE facts
include CPU/GPU integrals, energy, RAM maximum and byte-time/present-time, network
bytes, and five per-resource duration/cause pairs. Every observed instance is kept.
Coverage and uncertainty tables are counted even when separate from facts. Dictionary
inserts include all process identities, including churn. No top-N or floor is used.

The cheapest measured candidate uses column-major packing and standard-library zlib
across a five-minute batch. This demonstrates a conventional lossless encoding; it
is not production engine/store integration or a proof of CPU feasibility. Steady
samples model the existing thirty-second process cadence through ten-second slots.
Ninety percent are CPU idle and ninety-nine percent GPU idle, with nonzero RAM kept.
Source-incomplete rows are included. Churn replaces five percent of instances per
thirty-second source sample, without dropping the old instances.

The final 1,000-process, 350-app measurements are saved in
`ux-history-feasibility-1000.jsonl`. Adding the separately measured current baseline:

| Candidate | Payload bytes/hour | WAL bytes/hour | Rows in first hour |
| --- | ---: | ---: | ---: |
| Stable instances, compressed | 2036104 | 6317080 | 2316 |
| Moderate churn, compressed | 2118218 | 7449232 | 8266 |

The stable case includes 1,350 initial identity rows, which do not recur hourly;
without them its recurring combined row count is 966. The churn case recovers 6,950
process identities after reopening the database. Initial baseline and new probe were
separate runs, so addition is an estimate of combined production cost, not a measured
integrated writer. SQLite page interactions may change it. Busy workloads, missing
source metadata and real collector values still require the production gate.

The original 1.2 MB/6 MB/1,400 limits fail. The user approved storage-write ceilings
of 4,000,000 payload bytes/hour, 12,000,000 WAL bytes/hour and 10,000 rows/hour.
`perf-budget.json` and D-119 record only that approval. CPU, collection cadence,
permissions and the retained-file cap were not changed.

## Retained process details and tiers

The thirty-day identity-only SQLite fill retained 4,320,950 instances and 350 apps:
157,722,613 logical bytes, 242,991,104 database bytes after checkpoint. It reopened and
verified the full identity count. Instance identity is never merged by process name.

A complete candidate fill retained a finest-resolution day, six days of minute
sum/max rollups, and twenty-three days of fifteen-minute sum/max rollups. The rollup
fixture computes app RAM maxima from simultaneous process sums before time merging.
It retains per-resource duration/cause fields and all process identities. Actual
SQLite database size including indexes after checkpoint was **445,186,048 bytes**.

| Table | Rows | Logical bytes |
| --- | ---: | ---: |
| usage_10s | 8640 | 64489356 |
| usage_1m | 8640 | 70135446 |
| usage_15m | 2208 | 42437899 |
| usage_apps | 350 | 7942 |
| usage_processes | 4320950 | 157714671 |
| usage_coverage | 19488 | 935424 |
| usage_uncertain | 0 | 0 |

This retained fill does not batch the finest-day buckets across commits, so that tier
cost is pessimistic compared with the hourly candidate. It uses repeated synthetic
hour profiles with remapped process IDs, not a month of captured user activity.
The uncertainty table was empty in this full fill; the hourly probe adds six rows/hour
(about 0.16 MB logical over thirty days). These limitations must accompany the result.
It is not a production retention or long-range latency pass.

A provisional **750 MB** total cap accommodates this 445 MB candidate plus the current
150 MB existing-history allowance (595 MB), below a 750 MB cap's 675 MB low-water.
A 500 MB total cap cannot cover that combination. The current 150 MB cap remains
unchanged until the user decides; the production implementation must measure again.
Coarsening older facts can reduce tier storage, but not the 243 MB identity-only cost.

## Verification and remaining gate

- All 95 schema tests passed, including TypeScript export and CBOR round trips.
- Native current-store scoped volume test passed with nonzero measured writes.
- Candidate files reopen and verify identities plus complete first-frame contents.
- No idle, production CPU, production query latency or integrated thirty-day claims.
- No frontend attribution work is unlocked by the candidate results alone.
- Production tests will use real SQLite and public engine/store queries, following
  ce:writing-tests and repository testing rules. Lifecycle, rank, concurrent memory,
  partial retained edges, uncertainty, reset/prune and comparison still need coverage.

## Subsequent authorization and bounded replies

The user approved the recommended **750 MB default cap**. Valid saved caps remain
unchanged; smaller caps may shorten actual retained history and must be disclosed.
Schema and store defaults now agree, with 750 added to valid size-limit options.

Process expansion is bounded to pages of at most 100 instances, with source-generated
opaque continuation bound to interval, app, ranking, epoch and snapshot revision.
Network uncertainty replies summarize at most the ten returned apps plus one remainder.
Comparison requests accept at most ten unique app identities; intervals are limited to
the existing thirty-day Timeline horizon. These bounds do not discard retained facts.

The schema validation tests were mutation-verified: raising the membership cap and
relaxing the page-size bound each caused the corresponding behavioral test to fail
with its expected assertion. Both mutations were restored. Production pagination
round trips and process-detail queries remain part of the pending store implementation.
