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
  idle_suspended: "Paused · no running containers",
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
export type ContainerSort =
  | "cpu"
  | "memory"
  | "name"
  | "restarts"
  | "network"
  | "io";
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
            : sort === "network"
              ? (b.metrics.network_rx_bps ?? -1) +
                (b.metrics.network_tx_bps ?? -1) -
                ((a.metrics.network_rx_bps ?? -1) +
                  (a.metrics.network_tx_bps ?? -1))
              : sort === "io"
                ? (b.metrics.block_read_bps ?? -1) +
                  (b.metrics.block_write_bps ?? -1) -
                  ((a.metrics.block_read_bps ?? -1) +
                    (a.metrics.block_write_bps ?? -1))
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
