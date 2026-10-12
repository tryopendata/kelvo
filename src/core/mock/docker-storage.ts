import type {
  DockerSnapshot,
  DockerStorageOperation,
  DockerStorageResource,
  DockerStorageReview,
  DockerStorageSnapshot,
} from "@core/generated/bindings";
import type { CommandResult, Transport } from "@core/transport";

type Methods = Pick<
  Transport,
  | "dockerStorageScan"
  | "dockerStoragePrepare"
  | "dockerStorageExecute"
  | "dockerStorageOperation"
>;
const ok = <T>(data: T): CommandResult<T> => ({ status: "ok", data });
const fail = (message: string): CommandResult<never> => ({
  status: "error",
  error: { kind: "invalid_argument", message },
});
const MiB = 1024 ** 2;
const key = (row: { kind: string; id: string }) => `${row.kind}:${row.id}`;

export function mockStorageSnapshot(
  snapshot: DockerSnapshot,
  now: number
): DockerStorageSnapshot {
  const base: DockerStorageResource = {
    kind: "image",
    id: "",
    name: "",
    project: null,
    size_bytes: null,
    reclaimable_bytes: null,
    references: [],
    age_ms: null,
    age_basis: null,
    database: null,
    mounts: [],
    auto_remove: null,
    can_delete: false,
    can_stop: false,
    reason: null,
    fingerprint: "mock",
  };
  const resources: DockerStorageResource[] = [
    ...snapshot.containers.map((row, index): DockerStorageResource => {
      const stopped = row.lifecycle === "exited";
      const autoRemove = index === 1;
      return {
        ...base,
        kind: "container",
        id: row.id,
        name: row.name,
        project: row.project,
        size_bytes: (index === 6 ? 840 : 20) * MiB,
        reclaimable_bytes: stopped ? 840 * MiB : null,
        age_ms: stopped ? 9 * 86_400_000 : null,
        age_basis: stopped ? "stopped" : null,
        database: index === 6 ? "PostgreSQL" : null,
        mounts: [
          {
            kind: "volume",
            name: "project-data",
            source: "/var/lib/docker/volumes/project-data/_data",
            destination: "/data",
          },
          {
            kind: "bind",
            name: null,
            source: "/Users/mock/project",
            destination: "/app",
          },
        ],
        auto_remove: autoRemove,
        can_delete: stopped,
        can_stop: row.lifecycle === "running" && !autoRemove,
        reason: autoRemove
          ? "AutoRemove can delete the container and anonymous volumes on Stop."
          : stopped
            ? null
            : "Running container",
        fingerprint: `${row.id}:${row.lifecycle}`,
      };
    }),
    {
      ...base,
      id: "sha256:old-image",
      name: "old-api:latest",
      size_bytes: 1800 * MiB,
      reclaimable_bytes: 340 * MiB,
      can_delete: true,
      fingerprint: "old-image",
    },
    {
      ...base,
      id: "sha256:shared-image",
      name: "nginx:alpine",
      size_bytes: 250 * MiB,
      references: snapshot.containers.map((row) => row.name),
      reason: "Referenced by containers, including stopped containers",
      fingerprint: "shared-image",
    },
    {
      ...base,
      kind: "volume",
      id: "old-database-data",
      name: "old-database-data",
      size_bytes: 3200 * MiB,
      reclaimable_bytes: 3200 * MiB,
      can_delete: true,
      fingerprint: "old-volume",
    },
    {
      ...base,
      kind: "volume",
      id: "project-data",
      name: "project-data",
      project: "project-2",
      size_bytes: 1200 * MiB,
      references: snapshot.containers
        .filter((row) => row.id === "container-6")
        .map((row) => row.name),
      database: snapshot.containers.some((row) => row.id === "container-6")
        ? "PostgreSQL"
        : null,
      reason: "Referenced by a stopped container",
      fingerprint: "referenced-volume",
    },
    {
      ...base,
      kind: "build_cache",
      id: "cache.1",
      name: "Cached build layer",
      size_bytes: 960 * MiB,
      age_ms: 32 * 86_400_000,
      age_basis: "last_used",
      reason: "Exact cache-ID cleanup has not been verified for this engine",
      fingerprint: "cache",
    },
  ];
  return {
    engine_id: snapshot.engine?.id ?? "",
    generation: snapshot.generation,
    observed_ms: now,
    complete: true,
    resources,
    categories: [
      { kind: "image", size_bytes: 1900 * MiB, reclaimable_bytes: null },
      {
        kind: "container",
        size_bytes: resources
          .filter((row) => row.kind === "container")
          .reduce((sum, row) => sum + (row.size_bytes ?? 0), 0),
        reclaimable_bytes: null,
      },
      { kind: "volume", size_bytes: 4400 * MiB, reclaimable_bytes: 3200 * MiB },
      { kind: "build_cache", size_bytes: 960 * MiB, reclaimable_bytes: null },
    ],
    warnings: [
      "Build cache deletion is unavailable until exact ID matching is verified for this engine.",
    ],
  };
}

export function createMockDockerStorage(
  now: () => number,
  getSnapshot: () => DockerSnapshot,
  stopContainer: (id: string) => void,
  deleteContainer: (id: string) => void
) {
  let cached: DockerStorageSnapshot | null = null;
  let resumedGeneration: number | null = null;
  let sequence = 0;
  const deleted = new Set<string>();
  const reviews = new Map<string, DockerStorageReview>();
  const operations = new Map<string, DockerStorageOperation>();
  let active: string | null = null;
  const ready = () => {
    const snapshot = getSnapshot();
    return (
      snapshot.settings.enabled &&
      snapshot.engine &&
      (snapshot.status === "connected" ||
        (snapshot.status === "idle_suspended" &&
          resumedGeneration === snapshot.generation))
    );
  };
  const read = () => {
    const result = mockStorageSnapshot(getSnapshot(), now());
    result.resources = result.resources.filter((row) => !deleted.has(key(row)));
    // Mock daemon category totals change only by non-overlapping removed volumes/layers.
    for (const category of result.categories) {
      if (category.kind === "volume") {
        category.size_bytes = result.resources
          .filter((row) => row.kind === "volume")
          .reduce((sum, row) => sum + (row.size_bytes ?? 0), 0);
        category.reclaimable_bytes = result.resources
          .filter((row) => row.kind === "volume" && row.can_delete)
          .reduce((sum, row) => sum + (row.size_bytes ?? 0), 0);
      }
      if (category.kind === "image" && deleted.has("image:sha256:old-image"))
        category.size_bytes = 1560 * MiB;
    }
    return result;
  };
  const pruneOperations = () => {
    for (const [id, operation] of operations)
      if (
        operation.finished_ms !== null &&
        operation.finished_ms < now() - 3_600_000
      )
        operations.delete(id);
    while (operations.size > 20) {
      const oldest = [...operations].find(
        ([, value]) => value.finished_ms !== null
      );
      if (!oldest) break;
      operations.delete(oldest[0]);
    }
  };
  const methods: Methods = {
    async dockerStorageScan(force) {
      pruneOperations();
      const snapshot = getSnapshot();
      if (
        cached?.generation !== snapshot.generation ||
        cached.engine_id !== snapshot.engine?.id
      )
        cached = null;
      if (!ready())
        return ok({
          snapshot: cached,
          latest_operation_id:
            [...operations.values()]
              .filter((op) => op.engine_id === snapshot.engine?.id)
              .at(-1)?.id ?? null,
          error: "Resume monitoring before scanning storage.",
        });
      if (!cached || force || now() - cached.observed_ms >= 300_000)
        cached = read();
      return ok({
        snapshot: structuredClone(cached),
        error: null,
        latest_operation_id:
          [...operations.values()]
            .filter((op) => op.engine_id === snapshot.engine?.id)
            .at(-1)?.id ?? null,
      });
    },
    async dockerStoragePrepare(selections) {
      if (!ready()) return fail("Resume monitoring before reviewing actions.");
      if (active) return fail("An operation is already running.");
      if (
        !selections.length ||
        selections.length > 128 ||
        new Set(selections.map(key)).size !== selections.length
      )
        return fail("Select between 1 and 128 unique resources.");
      cached = read();
      const resources: DockerStorageResource[] = [];
      for (const selection of selections) {
        const row = cached.resources.find((row) => key(row) === key(selection));
        if (
          !row ||
          !(selection.action === "stop" ? row.can_stop : row.can_delete)
        )
          return fail("The selection changed or is no longer eligible.");
        resources.push(row);
      }
      const token = `review-${++sequence}`;
      const review: DockerStorageReview = {
        token,
        engine_id: cached.engine_id,
        generation: cached.generation,
        expires_ms: now() + 60_000,
        resources,
        selections: structuredClone(selections),
        estimated_reclaimed_bytes:
          resources.every(
            (row) =>
              (row.kind === "volume" || row.kind === "container") &&
              row.reclaimable_bytes !== null
          ) && selections.every((item) => item.action === "delete")
            ? resources.reduce(
                (sum, row) => sum + (row.reclaimable_bytes ?? 0),
                0
              )
            : null,
        warnings: resources.some((row) => row.kind === "volume")
          ? [
              "Volume deletion permanently removes data. Names can be reused by another Docker client.",
            ]
          : [],
      };
      reviews.set(token, review);
      return ok(structuredClone(review));
    },
    async dockerStorageExecute(token, confirmVolumes) {
      pruneOperations();
      for (const [id, prepared] of reviews)
        if (prepared.expires_ms <= now()) reviews.delete(id);
      if (active) return fail("An operation is already running.");
      const review = reviews.get(token);
      if (!review || review.expires_ms <= now())
        return fail(
          "Review expired or already consumed. Prepare another review."
        );
      if (
        review.resources.some((row) => row.kind === "volume") &&
        !confirmVolumes
      )
        return fail("Volume deletion requires explicit acknowledgement.");
      reviews.delete(token);
      if (
        !ready() ||
        review.generation !== getSnapshot().generation ||
        review.engine_id !== getSnapshot().engine?.id
      )
        return fail("Engine changed. Prepare another review.");
      const operation: DockerStorageOperation = {
        id: `operation-${++sequence}`,
        engine_id: review.engine_id,
        generation: review.generation,
        started_ms: now(),
        finished_ms: null,
        results: [],
        total: review.selections.length,
      };
      operations.set(operation.id, operation);
      active = operation.id;
      const response = structuredClone(operation);
      queueMicrotask(() => {
        for (const selection of review.selections) {
          const previous = review.resources.find(
            (row) => key(row) === key(selection)
          );
          const current = read().resources.find(
            (row) => key(row) === key(selection)
          );
          const eligible =
            ready() &&
            getSnapshot().generation === review.generation &&
            current &&
            current.fingerprint === previous?.fingerprint &&
            (selection.action === "stop"
              ? current.can_stop
              : current.can_delete);
          if (!eligible)
            operation.results.push({
              selection,
              status: "skipped",
              message: "Resource or engine changed after review",
              reclaimed_bytes: null,
            });
          else {
            if (selection.action === "stop") stopContainer(selection.id);
            else {
              deleted.add(key(selection));
              if (selection.kind === "container") deleteContainer(selection.id);
            }
            operation.results.push({
              selection,
              status: "succeeded",
              message:
                selection.action === "stop"
                  ? "Container stopped"
                  : "Resource removed",
              reclaimed_bytes: null,
            });
          }
        }
        operation.finished_ms = now();
        reviews.clear();
        active = null;
        cached = null;
        pruneOperations();
      });
      return ok(response);
    },
    async dockerStorageOperation(id) {
      pruneOperations();
      const operation = operations.get(id);
      return operation
        ? ok(structuredClone(operation))
        : fail("Operation is no longer retained.");
    },
  };
  return {
    methods,
    resume: () => {
      resumedGeneration = getSnapshot().generation;
    },
  };
}
