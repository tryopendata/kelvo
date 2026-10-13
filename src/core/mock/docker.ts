import type {
  DockerComparisonPage,
  DockerContainer,
  DockerEngine,
  DockerEvent,
  DockerHistoryPage,
  DockerHistoryPoint,
  DockerHistoryRequest,
  DockerSettings,
  DockerSnapshot,
} from "@core/generated/bindings";
import type { CommandResult, Transport } from "@core/transport";
import { createMockDockerStorage } from "./docker-storage";
import { MOCK_HOST_ID, type ScenarioName } from "./fixtures";

const ok = <T>(data: T): CommandResult<T> => ({ status: "ok", data });
export const MOCK_DOCKER_ENGINE = "mock-orbstack";
const MiB = 1024 ** 2;
const engine: DockerEngine = {
  id: MOCK_DOCKER_ENGINE,
  daemon_id: MOCK_DOCKER_ENGINE,
  runtime: "OrbStack",
  context: "orbstack",
  socket: "/Users/mock/.orbstack/run/docker.sock",
  reachable: true,
  preferred: true,
  error: null,
  api_version: "1.47",
  capacity_bytes: 8 * 1024 ** 3,
  capacity_observed_ms: 1_791_720_000_000,
};

export function mockContainer(index: number, now: number): DockerContainer {
  const measured = index < 128 && index % 7 !== 6;
  return {
    engine_id: engine.id,
    metadata_available: true,
    auto_remove: index === 1,
    metadata_observed_ms: now,
    id: `container-${index}`,
    name: index === 0 ? "worker" : index === 1 ? "web" : `service-${index}`,
    image:
      index === 0
        ? "api-worker:latest"
        : index === 6
          ? "postgres:16"
          : "nginx:alpine",
    project: index % 5 === 4 ? null : `project-${Math.floor(index / 5) + 1}`,
    service: index === 0 ? "worker" : "web",
    lifecycle: index % 7 === 6 ? "exited" : "running",
    health: index % 3 === 0 ? "no_check" : "healthy",
    restart_count: index === 0 ? 9 : 0,
    observed_restarts: index === 0 ? 3 : 0,
    repeated_restarts: index === 0,
    observed_restart_window_ms: 300_000,
    oom_killed: index === 0,
    explicit_limit_bytes: index === 0 ? 1024 * MiB : null,
    cpu_quota: null,
    restart_policy: "unless-stopped",
    ports: ["8080 → 80/tcp"],
    mounts: ["/app/data → /data (read-write)"],
    created: "2026-10-01T12:00:00Z",
    last_seen_ms: now,
    observed_ms: now,
    interval_ms: 1000,
    monitored: index < 128,
    metrics: {
      cpu_pct: measured ? (index === 0 ? 140 : 65 / (index + 1)) : null,
      working_set_bytes: measured ? (index === 0 ? 812 : 120) * MiB : null,
      raw_memory_bytes: measured ? (index === 0 ? 980 : 150) * MiB : null,
      effective_limit_bytes: index === 0 ? 1024 * MiB : 8 * 1024 * MiB,
      network_rx_bps: measured ? 12000 : null,
      network_tx_bps: measured ? 4500 : null,
      block_read_bps: measured ? 1000 : null,
      block_write_bps: measured ? 4000 : null,
    },
  };
}

function aggregateContainers(
  containers: DockerContainer[],
  now: number,
  interval = 1000
): DockerSnapshot["aggregate"] {
  const running = containers.filter((row) => row.lifecycle === "running");
  const measured = running.filter((row) => row.monitored);
  const fresh = measured.filter(
    (row) =>
      row.observed_ms !== null &&
      now - row.observed_ms <= Math.max(interval, 5000) * 2
  );
  const cpu = fresh.flatMap((row) =>
    row.metrics.cpu_pct === null ? [] : [row.metrics.cpu_pct]
  );
  const memory = fresh.flatMap((row) =>
    row.metrics.working_set_bytes === null
      ? []
      : [row.metrics.working_set_bytes]
  );
  return {
    cpu_cores: cpu.length ? cpu.reduce((a, b) => a + b, 0) / 100 : null,
    working_set_bytes: memory.length ? memory.reduce((a, b) => a + b, 0) : null,
    capacity_bytes: engine.capacity_bytes,
    cpu_measured: cpu.length,
    memory_measured: memory.length,
    expected: measured.length,
    running: running.length,
    total: containers.length,
    monitored: containers.filter((row) => row.monitored).length,
    monitoring_limit: 128,
    partial:
      measured.length < running.length ||
      cpu.length < measured.length ||
      memory.length < measured.length,
    inventory_complete: true,
  };
}
function projectAggregates(
  containers: DockerContainer[],
  now: number,
  interval = 1000
): DockerSnapshot["project_aggregates"] {
  return [...new Set(containers.map((row) => row.project))].map((project) => ({
    project,
    aggregate: aggregateContainers(
      containers.filter((row) => row.project === project),
      now,
      interval
    ),
  }));
}
export function mockDockerSnapshot(count: number, now: number): DockerSnapshot {
  const containers = Array.from({ length: count }, (_, i) => {
    const row = mockContainer(i, now);
    if (count !== 11) return row;
    const names = [
      "api",
      "worker",
      "caddy",
      "grafana-dev",
      "tempo",
      "redis",
      "postgres",
      "jobs",
      "proxy",
      "test",
      "builder",
    ];
    return {
      ...row,
      name: names[i] ?? row.name,
      project: i < 3 ? "liveoak" : i < 5 ? "opendata" : null,
      lifecycle: i < 5 ? ("running" as const) : ("exited" as const),
      health: "healthy" as const,
      oom_killed: false,
      repeated_restarts: false,
      observed_restarts: 0,
      restart_count: 0,
      auto_remove: false,
      metrics: {
        ...row.metrics,
        cpu_pct: i < 5 ? ([160, 70, 4, 3, 3][i] ?? 0) : null,
        working_set_bytes:
          i < 5 ? ([680, 498, 100, 400, 120][i] ?? 0) * MiB : null,
      },
    };
  });
  return {
    host_id: MOCK_HOST_ID,
    generation: 1,
    status: "connected",
    message: null,
    engine,
    settings: {
      enabled: true,
      selected_engine: engine.socket,
      show_popover: false,
      show_tray_count: false,
    },
    containers,
    aggregate: aggregateContainers(containers, now),
    project_aggregates: projectAggregates(containers, now),
    runtime: {
      scope: "shared",
      explanation:
        "OrbStack's runtime is shared with Linux machines. These macOS measurements are not additive with container memory.",
      cpu_pct: 265,
      footprint_bytes: 4.6 * 1024 ** 3,
      observed_ms: now,
      processes: ["OrbStack (pid 710)", "OrbStack Helper (pid 712)"],
    },
    coverage: [],
    observed_ms: now,
    now_ms: now,
    interval_ms: 1000,
    population_revision: 1,
    persistence: {
      status: "recording",
      error: null,
      logical_bytes: 8_000_000,
      allowance_bytes: 37_500_000,
      retained_from_ms: now - 86_400_000,
    },
  };
}

/** Shared generator: timestamp-positioned history with real gaps and partial buckets. */
export function mockHistoryPoints(
  request: DockerHistoryRequest,
  container: DockerContainer
): DockerHistoryPoint[] {
  const span = request.to_ms - request.from_ms;
  const tier =
    span <= 3_600_000 ? 10_000 : span <= 7 * 86_400_000 ? 60_000 : 900_000;
  const duration = Math.max(
    tier,
    Math.ceil(span / Math.max(1, request.max_points) / tier) * tier
  );
  const count = Math.min(request.max_points, Math.ceil(span / duration));
  const seed = [...container.id].reduce((n, c) => n + c.charCodeAt(0), 0);
  const base = container.metrics.cpu_pct ?? 20 + (seed % 65);
  const memory =
    container.metrics.working_set_bytes ?? (80 + (seed % 400)) * MiB;
  const from = Math.floor(request.from_ms / duration) * duration;
  const gapStart = from + Math.floor(count / 3) * duration;
  return Array.from({ length: count }, (_, i): DockerHistoryPoint | null => {
    const time = from + i * duration;
    if (time >= gapStart && time < gapStart + 60_000) return null;
    if (container.lifecycle === "removed" && i >= count / 2) return null;
    const cpu =
      i === 1
        ? 0
        : container.id === "removed-spike" && i === Math.floor(count / 4)
          ? 420
          : Math.max(0, base * (0.75 + 0.25 * Math.sin(i / 6 + seed)));
    const working = memory * (0.9 + 0.1 * Math.sin(i / 13 + seed));
    const partial =
      i === Math.floor(count * 0.65) || i === Math.floor(count * 0.8);
    const weight = partial ? Math.round(duration * 0.7) : duration - 1;
    const metrics = {
      ...container.metrics,
      cpu_pct: cpu,
      working_set_bytes: working,
    };
    const multiplier =
      duration > 10_000 && i === Math.floor(count / 4)
        ? 3
        : 1.05 + ((seed + i) % 36) / 100;
    return {
      time_ms: time,
      duration_ms: duration,
      expected_ms: duration,
      cpu_measured_ms: weight,
      memory_measured_ms: weight,
      cpu_covered: !partial,
      memory_covered: !partial,
      metrics,
      peak: {
        ...metrics,
        cpu_pct: cpu * multiplier,
        working_set_bytes: working * multiplier,
      },
      minimum: {
        ...metrics,
        cpu_pct: cpu * 0.8,
        working_set_bytes: working * 0.8,
      },
      population_revision: 1,
      revision: i,
      explicit_limit_bytes: container.explicit_limit_bytes,
      engine_capacity_bytes: engine.capacity_bytes,
    };
  }).filter((point): point is DockerHistoryPoint => point !== null);
}

type Methods = Pick<
  Transport,
  | "dockerRetained"
  | "dockerDiscover"
  | "dockerConfigure"
  | "dockerSnapshot"
  | "dockerSubscribe"
  | "dockerDetail"
  | "dockerHistory"
  | "dockerComparison"
  | "dockerEvents"
  | "dockerLogs"
  | "dockerResume"
>;
export function createMockDocker(
  scenarios: readonly ScenarioName[],
  now: () => number,
  onSettings?: (settings: DockerSettings) => void
) {
  const fixture = scenarios.find((s) => /^docker-(0|1|11|32|128|200)$/.test(s));
  const count = fixture ? Number(fixture.slice(7)) : 32;
  const startedAt = now();
  let snapshot = mockDockerSnapshot(count, now());
  const hasHistory = scenarios.some((s) => s.startsWith("docker-"));
  const enabled = hasHistory && !scenarios.includes("docker-retained");
  let retained = hasHistory
    ? [
        ...snapshot.containers,
        {
          ...mockContainer(0, now() - 60_000),
          id: "removed",
          name: "Removed worker",
          lifecycle: "removed" as const,
        },
        {
          ...mockContainer(2, now() - 60_000),
          id: "removed-spike",
          name: "retired-worker",
          project: "liveoak",
          lifecycle: "removed" as const,
        },
        {
          ...mockContainer(0, now() - 3_600_000),
          engine_id: "retained-colima",
          id: "old-worker",
          name: "Previous worker",
        },
      ]
    : [];
  if (!enabled)
    snapshot = {
      ...snapshot,
      status: "disabled",
      engine: null,
      settings: { ...snapshot.settings, enabled: false },
      containers: [],
      aggregate: aggregateContainers([], now()),
      project_aggregates: [],
    };
  if (scenarios.includes("docker-unavailable"))
    snapshot = {
      ...snapshot,
      status: "unavailable",
      message:
        "The selected engine is unavailable. Its history is still available.",
    };
  if (scenarios.includes("docker-idle"))
    snapshot = {
      ...mockDockerSnapshot(0, now()),
      status: "idle_suspended",
      interval_ms: 0,
      population_revision: 0,
      runtime: {
        scope: "shared",
        explanation:
          "OrbStack shares its runtime with other workloads. Docker-only process attribution has not been verified.",
        cpu_pct: null,
        footprint_bytes: null,
        observed_ms: null,
        processes: [],
      },
      message: "No running containers. Monitoring paused to save power.",
    };
  if (scenarios.includes("docker-save-failed"))
    snapshot.persistence = {
      ...snapshot.persistence,
      status: "write_failed",
      error: "History storage is temporarily full",
    };
  if (scenarios.includes("docker-history-gap"))
    snapshot.coverage.push({
      start_ms: now() - 300_000,
      end_ms: now() - 240_000,
      container_id: null,
      reason: "app_not_running",
    });
  if (scenarios.includes("docker-events-gap"))
    snapshot.coverage.push({
      start_ms: now() - 180_000,
      end_ms: now() - 120_000,
      container_id: null,
      reason: "events_disconnected",
    });
  let visible = true;
  const listeners = new Set<(s: DockerSnapshot) => void>();
  const logTimers = new Set<ReturnType<typeof setInterval>>();
  const emit = () => {
    snapshot = { ...snapshot, now_ms: now() };
    if (visible) for (const cb of listeners) cb(snapshot);
  };
  const storage = createMockDockerStorage(
    now,
    () => snapshot,
    (id) => {
      snapshot = {
        ...snapshot,
        containers: snapshot.containers.map((row) =>
          row.id === id
            ? {
                ...row,
                lifecycle: "exited",
                metrics: {
                  ...row.metrics,
                  cpu_pct: null,
                  working_set_bytes: null,
                },
              }
            : row
        ),
      };
      snapshot.aggregate = aggregateContainers(
        snapshot.containers,
        now(),
        snapshot.interval_ms
      );
      snapshot.project_aggregates = projectAggregates(
        snapshot.containers,
        now(),
        snapshot.interval_ms
      );
      emit();
    },
    (id) => {
      snapshot = {
        ...snapshot,
        containers: snapshot.containers.filter((row) => row.id !== id),
      };
      snapshot.aggregate = aggregateContainers(
        snapshot.containers,
        now(),
        snapshot.interval_ms
      );
      snapshot.project_aggregates = projectAggregates(
        snapshot.containers,
        now(),
        snapshot.interval_ms
      );
      emit();
    },
    (id) => {
      snapshot = {
        ...snapshot,
        containers: snapshot.containers.map((row, index) =>
          row.id === id
            ? {
                ...row,
                lifecycle: "running",
                metrics: mockContainer(index, now()).metrics,
              }
            : row
        ),
      };
      snapshot.aggregate = aggregateContainers(
        snapshot.containers,
        now(),
        snapshot.interval_ms
      );
      snapshot.project_aggregates = projectAggregates(
        snapshot.containers,
        now(),
        snapshot.interval_ms
      );
      emit();
    },
    scenarios.includes("docker-stop-failed")
  );
  const methods: Methods = {
    async dockerRetained(request) {
      const needle = request.search.toLocaleLowerCase();
      const matches = retained.filter(
        (row) =>
          (!request.engine_id || row.engine_id === request.engine_id) &&
          `${row.name} ${row.image} ${row.project ?? ""}`
            .toLocaleLowerCase()
            .includes(needle)
      );
      const limit = Math.min(100, Math.max(1, request.limit));
      return ok({
        engines: [...new Set(retained.map((row) => row.engine_id))].map(
          (id) => ({ id, runtime: id === engine.id ? "OrbStack" : "Colima" })
        ),
        containers: matches.slice(request.offset, request.offset + limit),
        total: matches.length,
        next_offset:
          request.offset + limit < matches.length
            ? request.offset + limit
            : null,
      });
    },
    async dockerDiscover() {
      return ok([
        engine,
        {
          ...engine,
          id: "mock-colima",
          runtime: "Colima",
          context: "colima",
          socket: "/Users/mock/.colima/default/docker.sock",
          preferred: false,
          reachable: false,
          error: "Socket is not available",
        },
      ]);
    },
    async dockerSnapshot() {
      return ok(snapshot);
    },
    async dockerConfigure(settings: DockerSettings) {
      if (
        settings.enabled === snapshot.settings.enabled &&
        settings.selected_engine === snapshot.settings.selected_engine
      ) {
        snapshot = { ...snapshot, settings };
        onSettings?.(settings);
        emit();
        return ok(snapshot);
      }
      snapshot = {
        ...snapshot,
        settings,
        generation: snapshot.generation + 1,
        status: settings.enabled ? "connected" : "disabled",
        engine: settings.enabled ? engine : null,
        containers: settings.enabled
          ? mockDockerSnapshot(count, now()).containers
          : [],
      };
      for (const row of snapshot.containers) {
        const index = retained.findIndex(
          (previous) =>
            previous.id === row.id && previous.engine_id === row.engine_id
        );
        if (index === -1) retained.push(row);
        else retained[index] = row;
      }
      snapshot.aggregate = aggregateContainers(
        snapshot.containers,
        now(),
        snapshot.interval_ms
      );
      snapshot.project_aggregates = projectAggregates(
        snapshot.containers,
        now(),
        snapshot.interval_ms
      );
      onSettings?.(settings);
      emit();
      return ok(snapshot);
    },
    async dockerSubscribe(cb) {
      listeners.add(cb);
      if (visible) cb(snapshot);
      return {
        info: ok(1),
        unsubscribe: () => {
          listeners.delete(cb);
        },
      };
    },
    async dockerDetail(engineId, containerId) {
      const row = snapshot.containers.find(
        (c) => c.id === containerId && c.engine_id === engineId
      );
      if (row) {
        snapshot = { ...snapshot, now_ms: now() };
        emit();
        return ok(row);
      }
      const saved = retained.find(
        (row) => row.engine_id === engineId && row.id === containerId
      );
      if (saved) return ok(saved);
      return {
        status: "error",
        error: {
          kind: "invalid_argument",
          message: "No retained container metadata",
        },
      };
    },
    async dockerHistory(request) {
      const metadata = retained.filter(
        (row) =>
          row.engine_id === request.engine_id &&
          (!request.container_id || row.id === request.container_id)
      );
      const page: DockerHistoryPage = {
        generation: snapshot.generation,
        coverage: [],
        metadata,
        persistence: snapshot.persistence,
        points: metadata[0] ? mockHistoryPoints(request, metadata[0]) : [],
      };
      return ok(page);
    },
    async dockerComparison(request) {
      if (
        request.max_points < 1 ||
        request.max_points > 360 ||
        (request.selected?.length ?? 0) > 6 ||
        request.from_ms >= request.to_ms
      )
        return {
          status: "error",
          error: {
            kind: "invalid_argument",
            message:
              "Comparison requires 1–360 points, at most six containers and a valid range.",
          },
        };
      const candidates = retained.filter(
        (row) =>
          row.engine_id === request.engine_id &&
          (request.project === null || (row.project ?? "") === request.project)
      );
      const all = candidates
        .map((container) => {
          const boosted =
            scenarios.includes("docker-rerank") &&
            now() - startedAt >= 15_000 &&
            container.id === "container-4"
              ? {
                  ...container,
                  metrics: {
                    ...container.metrics,
                    cpu_pct: 450,
                    working_set_bytes: 2_000 * MiB,
                  },
                }
              : container;
          const points = mockHistoryPoints(
            { ...request, container_id: container.id },
            boosted
          );
          const cpu = points.filter(
            (p) => p.cpu_measured_ms > 0 && p.metrics.cpu_pct !== null
          );
          const memory = points.filter(
            (p) =>
              p.memory_measured_ms > 0 && p.metrics.working_set_bytes !== null
          );
          const cpuWeight = cpu.reduce((sum, p) => sum + p.cpu_measured_ms, 0);
          const memoryWeight = memory.reduce(
            (sum, p) => sum + p.memory_measured_ms,
            0
          );
          return {
            container,
            points,
            peak_cpu_pct: cpu.length
              ? Math.max(...cpu.map((p) => p.peak.cpu_pct ?? 0))
              : null,
            avg_cpu_pct: cpuWeight
              ? cpu.reduce(
                  (sum, p) =>
                    sum + (p.metrics.cpu_pct ?? 0) * p.cpu_measured_ms,
                  0
                ) / cpuWeight
              : null,
            peak_working_set_bytes: memory.length
              ? Math.max(...memory.map((p) => p.peak.working_set_bytes ?? 0))
              : null,
            avg_working_set_bytes: memoryWeight
              ? memory.reduce(
                  (sum, p) =>
                    sum +
                    (p.metrics.working_set_bytes ?? 0) * p.memory_measured_ms,
                  0
                ) / memoryWeight
              : null,
            cpu_measured_ms: cpuWeight,
            memory_measured_ms: memoryWeight,
            last_lifecycle: container.lifecycle,
          };
        })
        .filter((row) =>
          request.metric === "cpu"
            ? row.cpu_measured_ms > 0
            : row.memory_measured_ms > 0
        );
      const value = (row: (typeof all)[number]) =>
        request.metric === "cpu"
          ? row.peak_cpu_pct
          : row.peak_working_set_bytes;
      all.sort(
        (a, b) =>
          (value(b) ?? -Infinity) - (value(a) ?? -Infinity) ||
          a.container.id.localeCompare(b.container.id)
      );
      const ranking = all.slice(0, 500);
      const selected =
        request.selected ??
        ranking
          .slice(0, ranking.length <= 6 ? 6 : 5)
          .map((row) => row.container.id);
      const page: DockerComparisonPage = {
        generation: snapshot.generation,
        ranking: ranking.map(({ points: _points, ...row }) => row),
        series: selected.flatMap((id) => {
          const row = all.find((row) => row.container.id === id);
          return row ? [{ container: row.container, points: row.points }] : [];
        }),
        coverage: snapshot.coverage,
        persistence: snapshot.persistence,
      };
      return ok(page);
    },
    async dockerEvents(request) {
      const events: DockerEvent[] = ["start", "oom", "die", "restart"].map(
        (action, i) => ({
          id: `docker-event-${i}`,
          engine_id: request.engine_id,
          container_id: request.container_id ?? "container-0",
          time_ms: request.to_ms - (4 - i) * 30_000,
          time_ns: `${request.to_ms - (4 - i) * 30_000}000000`,
          action,
          attributes: {},
          name: "worker",
          exit_code: action === "die" ? "137" : null,
        })
      );
      return ok({
        events: events.filter((e) => e.time_ms >= request.from_ms),
        next_id: null,
        coverage: [],
      });
    },
    async dockerLogs(request, cb) {
      cb({
        generation: snapshot.generation,
        lines: Array.from(
          { length: 200 },
          (_, i) =>
            `${i === 0 ? "<script>literal text</script>" : "worker"} · processed job ${i}`
        ),
        truncated: false,
        complete: !request.follow,
        error: null,
      });
      const timer = request.follow
        ? setInterval(
            () =>
              cb({
                generation: snapshot.generation,
                lines: [`worker · processed job at ${now()}`],
                truncated: false,
                complete: false,
                error: null,
              }),
            1000
          )
        : null;
      if (timer) logTimers.add(timer);
      return {
        info: ok(1),
        unsubscribe: () => {
          if (timer) {
            clearInterval(timer);
            logTimers.delete(timer);
          }
        },
      };
    },
    async dockerResume() {
      storage.resume();
      snapshot = { ...snapshot, status: "connected", message: null };
      emit();
      return ok(snapshot);
    },
  };
  return {
    methods: { ...methods, ...storage.methods },
    resetHistory() {
      retained = [];
      snapshot = {
        ...snapshot,
        generation: snapshot.generation + 1,
        containers: [],
        coverage: [],
        aggregate: aggregateContainers([], now()),
        project_aggregates: [],
      };
      emit();
    },
    setVisible(value: boolean) {
      visible = value;
      if (visible) emit();
      else {
        for (const timer of logTimers) clearInterval(timer);
        logTimers.clear();
      }
    },
    tick() {
      if (snapshot.settings.enabled && snapshot.status === "connected") {
        const containers = snapshot.containers.map((row, index) => ({
          ...row,
          observed_ms: now(),
          metrics: {
            ...row.metrics,
            cpu_pct:
              row.metrics.cpu_pct === null
                ? null
                : scenarios.includes("docker-rerank") &&
                    now() - startedAt >= 15_000 &&
                    row.id === "container-4"
                  ? 450
                  : (count === 11
                      ? ([160, 70, 4, 3, 3][index] ?? 0)
                      : index === 0
                        ? 140
                        : 65 / (index + 1)) + Math.sin(now() / 1000 + index),
          },
        }));
        snapshot = {
          ...snapshot,
          containers,
          aggregate: aggregateContainers(
            containers,
            now(),
            snapshot.interval_ms
          ),
          project_aggregates: projectAggregates(
            containers,
            now(),
            snapshot.interval_ms
          ),
          observed_ms: now(),
          now_ms: now(),
        };
        emit();
      }
    },
    dispose() {
      listeners.clear();
      for (const timer of logTimers) clearInterval(timer);
    },
  };
}
