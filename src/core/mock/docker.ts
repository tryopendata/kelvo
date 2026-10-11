import type {
  DockerContainer,
  DockerEngine,
  DockerEvent,
  DockerHistoryPage,
  DockerSettings,
  DockerSnapshot,
} from "@core/generated/bindings";
import type { CommandResult, Transport } from "@core/transport";
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
    metadata_observed_ms: now,
    id: `container-${index}`,
    name: index === 0 ? "worker" : index === 1 ? "web" : `service-${index}`,
    image: index === 0 ? "api-worker:latest" : "nginx:alpine",
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
  containers: DockerContainer[]
): DockerSnapshot["aggregate"] {
  const running = containers.filter((row) => row.lifecycle === "running");
  const measured = running.filter((row) => row.monitored);
  const cpu = measured.flatMap((row) =>
    row.metrics.cpu_pct === null ? [] : [row.metrics.cpu_pct]
  );
  const memory = measured.flatMap((row) =>
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
export function mockDockerSnapshot(count: number, now: number): DockerSnapshot {
  const containers = Array.from({ length: count }, (_, i) =>
    mockContainer(i, now)
  );
  return {
    host_id: MOCK_HOST_ID,
    generation: 1,
    status: "connected",
    message: null,
    engine,
    settings: {
      enabled: true,
      explicit_selection: false,
      selected_engine: engine.socket,
      monitored_containers: [],
      show_popover: false,
      show_tray_count: false,
    },
    containers,
    aggregate: aggregateContainers(containers),
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
    interval_ms: 1000,
    population_revision: 1,
    persistence: {
      status: "recording",
      logical_bytes: 8_000_000,
      allowance_bytes: 37_500_000,
      retained_from_ms: now - 86_400_000,
    },
  };
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
  | "dockerEvents"
  | "dockerLogs"
  | "dockerResume"
>;
export function createMockDocker(
  scenarios: readonly ScenarioName[],
  now: () => number,
  onSettings?: (settings: DockerSettings) => void
) {
  const fixture = scenarios.find((s) => /^docker-(0|1|32|128|200)$/.test(s));
  const count = fixture ? Number(fixture.slice(7)) : 32;
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
      ...snapshot,
      status: "idle_suspended",
      message:
        "Monitoring is suspended to let the runtime sleep. Resume to check for containers.",
    };
  let visible = true;
  const listeners = new Set<(s: DockerSnapshot) => void>();
  const logTimers = new Set<ReturnType<typeof setInterval>>();
  const emit = () => {
    if (visible) for (const cb of listeners) cb(snapshot);
  };
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
      if (settings.explicit_selection)
        snapshot.containers = snapshot.containers.map((row) => ({
          ...row,
          monitored: (settings.monitored_containers ?? []).includes(row.id),
        }));
      for (const row of snapshot.containers) {
        const index = retained.findIndex(
          (previous) =>
            previous.id === row.id && previous.engine_id === row.engine_id
        );
        if (index === -1) retained.push(row);
        else retained[index] = row;
      }
      snapshot.aggregate = aggregateContainers(snapshot.containers);
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
      if (row) return ok(row);
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
      const count = metadata.length ? Math.min(request.max_points, 180) : 0;
      const duration = Math.max(
        1000,
        Math.ceil((request.to_ms - request.from_ms) / Math.max(1, count))
      );
      const page: DockerHistoryPage = {
        generation: snapshot.generation,
        coverage: [],
        metadata,
        persistence: snapshot.persistence,
        points: Array.from({ length: count }, (_, i) => ({
          time_ms: request.from_ms + i * duration,
          duration_ms: duration,
          metrics: {
            ...mockContainer(0, now()).metrics,
            cpu_pct: 100 + Math.sin(i / 8) * 50,
            working_set_bytes: (400 + i * 3) * MiB,
          },
          measured_ms: i === 90 ? 0 : duration,
          expected_ms: duration,
          population_revision: 1,
          revision: i,
          explicit_limit_bytes: (i < 80 ? 768 : 1024) * MiB,
          engine_capacity_bytes: engine.capacity_bytes,
        })),
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
      snapshot = { ...snapshot, status: "connected", message: null };
      emit();
      return ok(snapshot);
    },
  };
  return {
    methods,
    resetHistory() {
      retained = [];
      snapshot = {
        ...snapshot,
        generation: snapshot.generation + 1,
        containers: [],
        coverage: [],
        aggregate: aggregateContainers([]),
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
      if (snapshot.settings.enabled) {
        const containers = snapshot.containers.map((row, index) => ({
          ...row,
          observed_ms: now(),
          metrics: {
            ...row.metrics,
            cpu_pct:
              row.metrics.cpu_pct === null
                ? null
                : (index === 0 ? 140 : 65 / (index + 1)) +
                  Math.sin(now() / 1000 + index),
          },
        }));
        snapshot = {
          ...snapshot,
          containers,
          aggregate: aggregateContainers(containers),
          observed_ms: now(),
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
