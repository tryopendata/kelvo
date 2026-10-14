import type { DockerContainer, DockerSnapshot } from "@core/generated/bindings";
import {
  aggregateContainers,
  mockContainer,
  mockDockerSnapshot,
} from "./docker-fixtures";
import type { ScenarioName } from "./fixtures";
export function retainedContainers(
  snapshot: DockerSnapshot,
  now: () => number
): DockerContainer[] {
  return [
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
  ];
}
export function scenarioSnapshot(
  snapshot: DockerSnapshot,
  scenarios: readonly ScenarioName[],
  enabled: boolean,
  now: () => number
): DockerSnapshot {
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
  return snapshot;
}
