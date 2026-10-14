import type {
  DockerContainer,
  DockerEvent,
  DockerHistoryPage,
  DockerSettings,
  DockerSnapshot,
} from "@core/generated/bindings";
import type { CommandResult, Transport } from "@core/transport";
import { comparison } from "./docker-comparison";
import {
  aggregateContainers,
  engine,
  mockContainer,
  mockDockerSnapshot,
  projectAggregates,
} from "./docker-fixtures";
import { mockHistoryPoints } from "./docker-history";
import { MockDockerLogs } from "./docker-logs";
import { retainedContainers, scenarioSnapshot } from "./docker-scenarios";
import { createMockDockerStorage } from "./docker-storage";
import type { ScenarioName } from "./fixtures";

export {
  MOCK_DOCKER_ENGINE,
  mockContainer,
  mockDockerSnapshot,
} from "./docker-fixtures";
export { mockHistoryPoints } from "./docker-history";

const ok = <T>(data: T): CommandResult<T> => ({ status: "ok", data });
export function createMockDocker(
  scenarios: readonly ScenarioName[],
  now: () => number,
  onSettings?: (settings: DockerSettings) => void
) {
  return new MockDocker(scenarios, now, onSettings);
}
class MockDocker {
  private snapshot: DockerSnapshot;
  private retained: DockerContainer[];
  private count: number;
  private startedAt: number;
  private visible = true;
  private listeners = new Set<(s: DockerSnapshot) => void>();
  private logs: MockDockerLogs;
  private storage: ReturnType<typeof createMockDockerStorage>;
  constructor(
    private readonly scenarios: readonly ScenarioName[],
    private readonly now: () => number,
    private readonly onSettings: ((s: DockerSettings) => void) | undefined
  ) {
    const fixture = scenarios.find((s) =>
      /^docker-(0|1|11|32|128|200)$/.test(s)
    );
    this.count = fixture ? Number(fixture.slice(7)) : 32;
    this.startedAt = now();
    const initial = mockDockerSnapshot(this.count, now());
    const hasHistory = scenarios.some((s) => s.startsWith("docker-"));
    this.retained = hasHistory ? retainedContainers(initial, now) : [];
    this.snapshot = scenarioSnapshot(
      initial,
      scenarios,
      hasHistory && !scenarios.includes("docker-retained"),
      now
    );
    this.logs = new MockDockerLogs(now, () => this.snapshot.generation);
    this.storage = this.createStorage();
  }
  private emit = () => {
    this.snapshot = { ...this.snapshot, now_ms: this.now() };
    if (this.visible) for (const cb of this.listeners) cb(this.snapshot);
  };
  private stopContainer = (id: string) => {
    this.snapshot = {
      ...this.snapshot,
      containers: this.snapshot.containers.map((row) =>
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
    this.snapshot.aggregate = aggregateContainers(
      this.snapshot.containers,
      this.now(),
      this.snapshot.interval_ms
    );
    this.snapshot.project_aggregates = projectAggregates(
      this.snapshot.containers,
      this.now(),
      this.snapshot.interval_ms
    );
    this.emit();
  };
  private removeContainer = (id: string) => {
    this.snapshot = {
      ...this.snapshot,
      containers: this.snapshot.containers.filter((row) => row.id !== id),
    };
    this.snapshot.aggregate = aggregateContainers(
      this.snapshot.containers,
      this.now(),
      this.snapshot.interval_ms
    );
    this.snapshot.project_aggregates = projectAggregates(
      this.snapshot.containers,
      this.now(),
      this.snapshot.interval_ms
    );
    this.emit();
  };
  private restartContainer = (id: string) => {
    this.snapshot = {
      ...this.snapshot,
      containers: this.snapshot.containers.map((row, index) =>
        row.id === id
          ? {
              ...row,
              lifecycle: "running",
              metrics: mockContainer(index, this.now()).metrics,
            }
          : row
      ),
    };
    this.snapshot.aggregate = aggregateContainers(
      this.snapshot.containers,
      this.now(),
      this.snapshot.interval_ms
    );
    this.snapshot.project_aggregates = projectAggregates(
      this.snapshot.containers,
      this.now(),
      this.snapshot.interval_ms
    );
    this.emit();
  };
  private createStorage() {
    return createMockDockerStorage(
      this.now,
      () => this.snapshot,
      this.stopContainer,
      this.removeContainer,
      this.restartContainer,
      this.scenarios.includes("docker-stop-failed")
    );
  }
  readonly dockerRetained: Transport["dockerRetained"] = async (request) => {
    const needle = request.search.toLocaleLowerCase();
    const matches = this.retained.filter(
      (row) =>
        (!request.engine_id || row.engine_id === request.engine_id) &&
        `${row.name} ${row.image} ${row.project ?? ""}`
          .toLocaleLowerCase()
          .includes(needle)
    );
    const limit = Math.min(100, Math.max(1, request.limit));
    return ok({
      engines: [...new Set(this.retained.map((row) => row.engine_id))].map(
        (id) => ({ id, runtime: id === engine.id ? "OrbStack" : "Colima" })
      ),
      containers: matches.slice(request.offset, request.offset + limit),
      total: matches.length,
      next_offset:
        request.offset + limit < matches.length ? request.offset + limit : null,
    });
  };
  readonly dockerDiscover: Transport["dockerDiscover"] = async () => {
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
  };
  readonly dockerSnapshot: Transport["dockerSnapshot"] = async () => {
    return ok(this.snapshot);
  };
  readonly dockerConfigure: Transport["dockerConfigure"] = async (
    settings: DockerSettings
  ) => {
    if (
      settings.enabled === this.snapshot.settings.enabled &&
      settings.selected_engine === this.snapshot.settings.selected_engine
    ) {
      this.snapshot = { ...this.snapshot, settings };
      this.onSettings?.(settings);
      this.emit();
      return ok(this.snapshot);
    }
    this.snapshot = {
      ...this.snapshot,
      settings,
      generation: this.snapshot.generation + 1,
      status: settings.enabled ? "connected" : "disabled",
      engine: settings.enabled ? engine : null,
      containers: settings.enabled
        ? mockDockerSnapshot(this.count, this.now()).containers
        : [],
    };
    for (const row of this.snapshot.containers) {
      const index = this.retained.findIndex(
        (previous) =>
          previous.id === row.id && previous.engine_id === row.engine_id
      );
      if (index === -1) this.retained.push(row);
      else this.retained[index] = row;
    }
    this.snapshot.aggregate = aggregateContainers(
      this.snapshot.containers,
      this.now(),
      this.snapshot.interval_ms
    );
    this.snapshot.project_aggregates = projectAggregates(
      this.snapshot.containers,
      this.now(),
      this.snapshot.interval_ms
    );
    this.onSettings?.(settings);
    this.emit();
    return ok(this.snapshot);
  };
  readonly dockerSubscribe: Transport["dockerSubscribe"] = async (cb) => {
    this.listeners.add(cb);
    if (this.visible) cb(this.snapshot);
    return {
      info: ok(1),
      unsubscribe: () => {
        this.listeners.delete(cb);
      },
    };
  };
  readonly dockerDetail: Transport["dockerDetail"] = async (
    engineId,
    containerId
  ) => {
    const row = this.snapshot.containers.find(
      (c) => c.id === containerId && c.engine_id === engineId
    );
    if (row) {
      this.snapshot = { ...this.snapshot, now_ms: this.now() };
      this.emit();
      return ok(row);
    }
    const saved = this.retained.find(
      (row) => row.engine_id === engineId && row.id === containerId
    );
    if (saved) return ok(saved);
    return {
      status: "error",
      error: {
        kind: "docker_not_found",
        message: "No retained container metadata",
      },
    };
  };
  readonly dockerHistory: Transport["dockerHistory"] = async (request) => {
    const metadata = this.retained.filter(
      (row) =>
        row.engine_id === request.engine_id &&
        (!request.container_id || row.id === request.container_id)
    );
    const page: DockerHistoryPage = {
      generation: this.snapshot.generation,
      coverage: [],
      metadata,
      persistence: this.snapshot.persistence,
      points: metadata[0] ? mockHistoryPoints(request, metadata[0]) : [],
    };
    return ok(page);
  };
  readonly dockerEvents: Transport["dockerEvents"] = async (request) => {
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
  };
  readonly dockerResume: Transport["dockerResume"] = async () => {
    this.storage.resume();
    this.snapshot = { ...this.snapshot, status: "connected", message: null };
    this.emit();
    return ok(this.snapshot);
  };
  readonly dockerLogs: Transport["dockerLogs"] = (...args) =>
    this.logs.subscribe(...args);
  readonly dockerComparison: Transport["dockerComparison"] = async (request) =>
    comparison(
      request,
      this.retained,
      this.snapshot,
      this.scenarios,
      this.now,
      this.startedAt
    );
  get methods() {
    return {
      dockerRetained: this.dockerRetained,
      dockerDiscover: this.dockerDiscover,
      dockerSnapshot: this.dockerSnapshot,
      dockerConfigure: this.dockerConfigure,
      dockerSubscribe: this.dockerSubscribe,
      dockerDetail: this.dockerDetail,
      dockerHistory: this.dockerHistory,
      dockerComparison: this.dockerComparison,
      dockerEvents: this.dockerEvents,
      dockerLogs: this.dockerLogs,
      dockerResume: this.dockerResume,
      ...this.storage.methods,
    };
  }
  resetHistory() {
    this.retained = [];
    this.snapshot = {
      ...this.snapshot,
      generation: this.snapshot.generation + 1,
      containers: [],
      coverage: [],
      aggregate: aggregateContainers([], this.now()),
      project_aggregates: [],
    };
    this.emit();
  }
  setVisible(value: boolean) {
    this.visible = value;
    if (this.visible) this.emit();
    else {
      this.logs.clear();
    }
  }
  private tickCpu(row: DockerContainer, index: number): number | null {
    if (row.metrics.cpu_pct === null) return null;
    if (
      this.scenarios.includes("docker-rerank") &&
      this.now() - this.startedAt >= 15_000 &&
      row.id === "container-4"
    )
      return 450;
    const base =
      this.count === 11
        ? ([160, 70, 4, 3, 3][index] ?? 0)
        : index === 0
          ? 140
          : 65 / (index + 1);
    return base + Math.sin(this.now() / 1000 + index);
  }
  tick() {
    if (
      this.snapshot.settings.enabled &&
      this.snapshot.status === "connected"
    ) {
      const containers = this.snapshot.containers.map((row, index) => ({
        ...row,
        observed_ms: this.now(),
        metrics: {
          ...row.metrics,
          cpu_pct: this.tickCpu(row, index),
        },
      }));
      this.snapshot = {
        ...this.snapshot,
        containers,
        aggregate: aggregateContainers(
          containers,
          this.now(),
          this.snapshot.interval_ms
        ),
        project_aggregates: projectAggregates(
          containers,
          this.now(),
          this.snapshot.interval_ms
        ),
        observed_ms: this.now(),
        now_ms: this.now(),
      };
      this.emit();
    }
  }
  dispose() {
    this.listeners.clear();
    this.logs.clear();
  }
}
