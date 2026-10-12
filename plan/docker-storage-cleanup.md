# Docker storage insights and cleanup

Status: Completed. D-107 extends the shipped local Docker monitor with explicit actions.
Real-runtime acceptance beyond OrbStack and a whole-app idle CPU measurement remain
explicit follow-ups below.

## Outcome and scope

Users can see what Docker-managed objects consume, review unused/stopped resources,
identify resource-heavy or unhealthy workloads, and explicitly clean selected items.
Add Activity and Storage & cleanup within Containers; preserve existing history/logs.
One selected local engine only. No database credentials, SQL inspection, scheduled
cleanup, network pruning, VM compaction, host-file deletion, or remote commands.

Storage means images, writable layers, named/anonymous volumes and daemon build cache.
It does not promise the runtime's allocated Mac footprint: VM files, logs, bind mounts
and other Buildx builders may add more. Shared layers make row sizes non-additive.
Use daemon/category accounting and nullable conservative selected-set estimates;
never add overlapping image/cache values into a purported exact total.

## Experience and evidence

- Activity keeps live metrics, history and logs, with resource rankings and a compact
  review list for existing health, OOM and restart evidence. Manual Stop is available
  for running containers except AutoRemove containers (explain why it is disabled).
- Storage leads with the breakdown and measured timestamp, then one searchable,
  sortable resource table: name/type, project, size, references, age evidence and
  eligibility. Largest first; project grouping. Nothing selected by default.
- Review filters: stopped containers, unused images, unreferenced volumes, old cache.
  Seven days is the default stopped/last-used threshold, with 1/7/30-day choices.
  Age applies only to actual stopped-container/cache-last-used evidence. Unused images
  and unreferenced volumes do not acquire invented unused ages from creation dates.
  Unknown-age rows remain accessible with an explicit all/unknown option.
- Infer likely database workloads from recognized image repositories and label this
  inference. Preserve structured mounts (type/name/source/destination), references
  from stopped containers, and AutoRemove. Orphan volumes without evidence say
  previous use unknown; no promise of identifying every abandoned database.
- Match existing tables, dialogs, segmented controls, tokens and type. No new card grid.

## Collection and interfaces

- Rust owns meaning, accounting, relationships, eligibility and classification. Add
  Rust DTOs and typed local operations for scan, prepare review, execute confirmation,
  and operation status. Regenerate bindings; extend the mock transport in the same change.
  Do not extend remote protocols or the live metric stream for storage snapshots.
- Share one on-demand scan per selected engine. On entering Storage refresh a cache
  older than five minutes; manual refresh also available. No background scan timer.
  Existing idle suspension requires Resume. Explicit Resume grants generation-scoped
  on-demand storage access even if a zero-running-container monitor immediately
  suspends again; a review prepared while connected grants the same access so stopping
  the last running container does not strand approved cleanup. Disable, sleep and
  engine changes revoke it. Cache snapshots in memory only.
- A separate scan client owns a 60-second request/cycle deadline (not the monitoring
  client's two-second timeout), one scan at a time, and the existing 16 MiB response
  ceiling. Keep finite row/request admission bounded; mark incomplete inventory and
  unknown categories. Failed scans retain the previous completed snapshot timestamp.
  Incomplete reference evidence cannot establish deletion eligibility.
- Dedicated scan/action lanes protect Kelvo's monitoring pool; daemon-side scan cost
  is measured and disclosed, not claimed to be eliminated.

## Actions, confirmation and lifecycle

- Preserve monitoring's GET-only client. Add a separate narrow action client with
  method/path/query allowlists; never expose arbitrary Docker calls or shell commands.
- Review selected stopped-container deletion, unused-image deletion, unreferenced-volume
  deletion and build-cache cleanup. No force deletion; container removal uses v=false.
  Do not automatically add newly eligible dependencies to a reviewed selection.
- Volume deletion has its own confirmation naming each volume and permanent data loss.
  Reinspect creation metadata, driver/options/labels and complete references; skip
  observed changes; use force=false. Docker volumes have reusable names and no atomic
  conditional-delete API: explicitly disclose that another Docker client replacing
  an unused volume between recheck and deletion cannot be detected reliably. Do not
  claim race-free identity protection. Bind-mounted host paths are never removed.
- Collect AutoRemove and disable Stop for those containers in this release, because
  stopping can implicitly delete the container and anonymous volumes. Ordinary Stop
  confirms a 30-second grace period followed by Docker termination if needed.
- Cache IDs are not assumed literal: known Moby Engine implementations translate the
  id filter into a regex and reject multiple values. Use one escaped anchored ID per
  request only for implementations whose exact matching has been verified by tests
  and real disposable fixtures. Otherwise report cleanup unsupported, never broad prune.
- Prepared reviews expire after 60 seconds and are atomically consumed once. Bind them
  to engine identity/socket, generation, resource metadata and the exact reviewed set.
  Revalidate before each dispatch; unavailable/changed/in-use resources are skipped.
- One Rust-owned operation per engine, bounded to 128 selected items. It survives UI
  navigation/closure. Engine/generation changes, disable, or sleep stop further dispatch;
  already sent requests settle or become outcome-unknown. Never retry a mutation
  automatically. Shutdown does not promise cancellation of daemon-side actions.
- Retain up to 20 operation results in memory; expire after one hour. App restart loses
  this session history: rescan before further action and never replay operations.
- Report success/skipped/failed/outcome-unknown per item and refresh after completion.
  Keep daemon-reported reclaimed bytes separate from observed before/after changes;
  deletion responses without a byte total remain unknown, and Mac free space is not promised.

## Execution tasks and dependencies

- [x] A. Contracts and adapter: DTOs, bounded scanning, structured metadata, narrow actions,
  classification/accounting and fake-daemon tests. Establish contracts before B/C.
- [x] B. Engine and shell (depends on A): scan cache, review/action lifecycle, local commands,
  generated bindings and lifecycle/race tests.
- [x] C. Frontend (depends on A/B contracts): transport/mock, Storage UI, Activity review and
  Stop, confirmation/results flows, component and browser tests.
- [x] D. Acceptance (depends on A/B/C): disposable real-engine validation, screenshot comparison,
  full gates, CE code review, fix/re-review, commit and merge; update this record and PROGRESS.

## Verification gates

Test shared ownership, unknown sizes/times, incomplete reference inventories, image inference,
stopped references, volume name replacement, AutoRemove, cache regex metacharacters and unrelated
records, expired/reused confirmations, engine transitions, partial batches and ambiguous timeouts.
Mutation-test eligibility/allowlist guards. Exercise empty/loading/error/unsupported UI flows,
confirm/cancel, volume warnings and refreshed results in Vitest and Playwright.

Compare disposable fixture accounting with docker system df -v; never delete pre-existing user
resources. Verify supported action behavior on available Docker Desktop, OrbStack and Colima,
and record unavailable runtimes explicitly. Light/dark screenshots must be compared against
Containers, Processes and the dev gallery; document intentional differences. Run required
frontend/Rust suites, regenerated binding checks, make check and existing performance gates.
Verify no background scans and measure daemon/monitoring effects without raising budgets.

## Review evidence

The CE devil's-advocate review identified reusable volume names, AutoRemove Stop side effects,
cache regex matching, operation lifecycle, inherited two-second client timeouts, unsupported
unused-age claims, orphan database inference and non-additive estimates. All are addressed above.

Sources: [Docker disk usage](https://docs.docker.com/reference/cli/docker/system/df/),
[anonymous volumes](https://docs.docker.com/engine/storage/volumes/#remove-anonymous-volumes),
[Moby API](https://github.com/moby/moby/blob/v28.1.0/docs/api/v1.49.yaml),
[Moby cache filtering](https://github.com/moby/moby/blob/v28.1.0/builder/builder-next/builder.go).

## Execution evidence

- OrbStack Engine Community 29.4.0 (daa0cb7f), API negotiated through 1.49:
  disposable engine scan/prepare/execute/status test passed; replay was rejected.
- Disposable adapter test passed normal Stop with its 30-second grace, volume delete,
  exact cache delete (4,096 bytes), and AutoRemove refusal. Cache assertions preserved
  every nonselected ID; independent comparison preserved all 76 pre-existing cache IDs,
  all 16 pre-existing images, and every non-fixture container/volume.
- A second disposable pass verified image deletion without pruning parents. All six
  newly created cache records were individually cleaned through exact-ID actions;
  a final inventory comparison showed no new fixture resources left and no
  pre-existing resources removed. Category estimates matched the daemon's disk-usage
  breakdown (about 472.5 MB stopped layers, 6.18 GB image estimate, 125.9 MB volumes
  and 1.72 GB private unused cache), with no physical Mac-space claim.
- Parent manually exercised mock Storage selection, two-stage volume acknowledgement,
  success/results refresh, Activity Stop confirmation and disabled AutoRemove Stop.
  Light/dark visual comparison uses existing Containers, Processes and gallery patterns.
- `make check` passed: Rust workspace tests, standard/App Store checks, Biome,
  TypeScript and 863 Rust tests. After review fixes, frontend validation passed
  all 987 tests in 160 files. The final full browser suite passed 294 checks with
  10 intentional skips across Chromium and WebKit; an earlier transient gallery
  navigation interruption did not recur. Existing Containers performance gates
  passed at 25.77 ms/s overview and 18.72 ms/s detail, below unchanged 35 ms/s
  limits, with no long tasks. Binding regeneration matched exactly.
- Independent CE review found and prompted fixes for mixed-batch final confirmation,
  recovering newer operations over older cached results (including late-response
  races), and mock review-consumption ordering. Four frontend regression mutations
  failed as expected; final confirmation was also manually checked with mixed
  image/volume selections. Re-review also caught and resolved mock completion invalidation and unbounded
  volume-name acknowledgement text. The final CE review approved all 33 changed
  files with no remaining findings. All eight Docker browser checks passed again
  after those fixes; a 128-long-name selection regression also passed.
- Deliberate visual additions are the Activity/Storage switch, a static category
  summary in place of the Activity chart, evidence disclosures, and explicit action
  review/results dialogs. They reuse existing tokens, table density and dialog
  patterns; no new dashboard card system or animated storage charts were introduced.
- Eligibility, AutoRemove, incomplete-reference, dispatch-generation, cache capability,
  review expiry/replay and external-volume accounting guards have mutation-test
  evidence. Cache success requires daemon confirmation of the selected ID.
- Docker Desktop and Colima are not configured on this machine; their real-runtime
  acceptance remains open. This does not establish a whole-app idle CPU measurement.
