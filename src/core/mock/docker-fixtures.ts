import type {
  DockerContainer,
  DockerEngine,
  DockerSnapshot,
} from "@core/generated/bindings";

import { MOCK_HOST_ID } from "./fixtures";

export const MOCK_DOCKER_ENGINE = "mock-orbstack";
export const MiB = 1024 ** 2;
export const engine: DockerEngine = {
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
  return {
    engine_id: engine.id,
    metadata_available: true,
    auto_remove: index === 1,
    metadata_observed_ms: now,
    id: `container-${index}`,
    name: ["worker", "web"][index] ?? `service-${index}`,
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
    metrics: containerMetrics(index),
  };
}

export function aggregateContainers(
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
export function projectAggregates(
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
      metrics: elevenContainerMetrics(row, i),
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

function containerMetrics(index: number): DockerContainer["metrics"] {
  const measured = index < 128 && index % 7 !== 6;
  return {
    cpu_pct: measured ? (index === 0 ? 140 : 65 / (index + 1)) : null,
    working_set_bytes: measured ? (index === 0 ? 812 : 120) * MiB : null,
    raw_memory_bytes: measured ? (index === 0 ? 980 : 150) * MiB : null,
    effective_limit_bytes: index === 0 ? 1024 * MiB : 8 * 1024 * MiB,
    network_rx_bps: measured ? 12000 : null,
    network_tx_bps: measured ? 4500 : null,
    block_read_bps: measured ? 1000 : null,
    block_write_bps: measured ? 4000 : null,
  };
}

function elevenContainerMetrics(
  row: DockerContainer,
  i: number
): DockerContainer["metrics"] {
  return {
    ...row.metrics,
    cpu_pct: i < 5 ? ([160, 70, 4, 3, 3][i] ?? 0) : null,
    working_set_bytes: i < 5 ? ([680, 498, 100, 400, 120][i] ?? 0) * MiB : null,
  };
}
