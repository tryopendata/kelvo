import type {
  CommandError,
  DockerContainer,
  DockerHealth,
  DockerLifecycle,
  DockerStatus,
} from "@core/generated/bindings";
export const statusLabel: Record<DockerStatus, string> = {
  disabled: "Monitoring off",
  connecting: "Connecting",
  connected: "Connected",
  idle_suspended: "Suspended for runtime sleep",
  paused: "Paused",
  unavailable: "Unavailable",
  permission_denied: "Socket permission denied",
  incompatible: "Incompatible engine API",
};
export const lifecycleLabel: Record<DockerLifecycle, string> = {
  created: "Created",
  running: "Running",
  paused: "Paused",
  restarting: "Restarting",
  exited: "Exited",
  removing: "Removing",
  dead: "Dead",
  removed: "Removed",
  unknown: "Unknown",
};
export const healthLabel: Record<DockerHealth, string> = {
  no_check: "No check",
  starting: "Starting",
  healthy: "Healthy",
  unhealthy: "Unhealthy",
  unknown: "Unknown",
};
export type ContainerSort = "cpu" | "memory" | "name" | "restarts";
export const containerPath = (engine: string, container: string) =>
  `/dashboard/containers/${encodeURIComponent(engine)}/${encodeURIComponent(container)}`;
export function sortedContainers(
  rows: readonly DockerContainer[],
  sort: ContainerSort
): DockerContainer[] {
  return [...rows].sort((a, b) => {
    const delta =
      sort === "name"
        ? a.name.localeCompare(b.name)
        : sort === "cpu"
          ? (b.metrics.cpu_pct ?? -1) - (a.metrics.cpu_pct ?? -1)
          : sort === "memory"
            ? (b.metrics.working_set_bytes ?? -1) -
              (a.metrics.working_set_bytes ?? -1)
            : (b.restart_count ?? -1) - (a.restart_count ?? -1);
    return delta || a.id.localeCompare(b.id);
  });
}
export function filterContainers(
  rows: readonly DockerContainer[],
  query: string,
  state: string
) {
  const needle = query.trim().toLocaleLowerCase();
  return rows.filter(
    (row) =>
      (state === "all" || row.lifecycle === state) &&
      `${row.name}\n${row.image}\n${row.project ?? ""}`
        .toLocaleLowerCase()
        .includes(needle)
  );
}

/** Keep bounded, actionable errors supplied by Rust visible at the UI seam. */
export function dockerErrorMessage(error: CommandError): string {
  return "message" in error ? error.message : error.kind.replaceAll("_", " ");
}

/** Inspection supplies metadata; the stream remains authoritative for live values. */
export function withInspection(
  live: DockerContainer | undefined,
  inspected: DockerContainer | undefined
): DockerContainer | undefined {
  if (!live) return inspected;
  if (!inspected) return live;
  const inspectedAt = inspected.metadata_observed_ms ?? null;
  const newerState = inspectedAt !== null && inspectedAt >= live.last_seen_ms;
  // A raced inspect 404 is newer removal evidence, including Rust's cleared
  // metrics. Inventory can still contain the row until its next refresh.
  if (newerState && inspected.lifecycle === "removed") return inspected;
  const newerMetadata =
    inspectedAt !== null &&
    (live.metadata_observed_ms == null ||
      inspectedAt >= live.metadata_observed_ms);
  const metadata =
    inspected.metadata_available && (!live.metadata_available || newerMetadata)
      ? inspected
      : live;
  return {
    ...live,
    lifecycle: newerState ? inspected.lifecycle : live.lifecycle,
    health: newerState ? inspected.health : live.health,
    metadata_available: metadata.metadata_available,
    metadata_observed_ms: metadata.metadata_observed_ms,
    restart_count: metadata.restart_count,
    oom_killed: metadata.oom_killed,
    explicit_limit_bytes: metadata.explicit_limit_bytes,
    cpu_quota: metadata.cpu_quota,
    restart_policy: metadata.restart_policy,
    ports: metadata.ports,
    mounts: metadata.mounts,
    created: metadata.created,
  };
}
