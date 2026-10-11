import { formatBytes, formatPercent } from "@core/format";
import { useMemo } from "react";
import { Link } from "react-router";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { useTransport } from "~/lib/transport-context";
import { useSettings } from "~/stores/settings-store";
import {
  useDocker,
  useDockerEvents,
  useDockerHistory,
} from "../_hooks/use-docker";
import {
  containerPath,
  sortedContainers,
  statusLabel,
} from "../_lib/presentation";
import { ContainerHistory } from "./container-history";

function LivePopoverSummary() {
  const { snapshot } = useDocker();
  const transport = useTransport();
  const rows = snapshot?.containers;
  const leader = useMemo(
    () =>
      sortedContainers(
        (rows ?? []).filter(
          (row) => row.monitored && row.metrics.cpu_pct !== null
        ),
        "cpu"
      )[0],
    [rows]
  );
  if (!snapshot) return null;
  return (
    <section
      aria-label="Container glance"
      className="mx-2.5 rounded-card border border-border bg-card-vibrant p-3"
    >
      <button
        type="button"
        onClick={() => void transport.openDashboard("/dashboard/containers")}
        className="block w-full rounded-control text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span className="flex justify-between text-[12px]">
          <span>Containers</span>
          <span className="figures">{snapshot.aggregate.running} running</span>
        </span>
        <span className="figures mt-1 block text-[11px] text-muted-foreground">
          {snapshot.aggregate.cpu_cores?.toFixed(2) ?? "—"} cores ·{" "}
          {formatBytes(snapshot.aggregate.working_set_bytes)} ·{" "}
          {statusLabel[snapshot.status]} ·{" "}
          {snapshot.aggregate.partial
            ? "Partial coverage"
            : "Complete coverage"}
          <span className="mt-1 block">
            Observed {new Date(snapshot.observed_ms).toLocaleTimeString()}
          </span>
        </span>
      </button>
      {leader && (
        <button
          type="button"
          className="mt-2 block rounded-control text-left text-[11px] text-link underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          onClick={() =>
            void transport.openDashboard(
              containerPath(leader.engine_id, leader.id)
            )
          }
        >
          {snapshot.status === "connected"
            ? "Highest CPU"
            : "Last observed highest CPU"}{" "}
          among monitored: {leader.name} ·{" "}
          {formatPercent(leader.metrics.cpu_pct)}
        </button>
      )}
    </section>
  );
}
export function DockerPopoverSummary() {
  const local = useIsLocalHost();
  const shown = useSettings(
    (settings) => settings.docker?.enabled && settings.docker?.show_popover
  );
  return local && shown ? <LivePopoverSummary /> : null;
}
function LiveTimelineLane({ from, to }: { from: number; to: number }) {
  const { snapshot } = useDocker();
  const engine = snapshot?.engine?.id ?? "";
  const history = useDockerHistory(
    engine,
    null,
    from,
    to,
    snapshot?.generation
  );
  const events = useDockerEvents(engine, null, from, to, snapshot?.generation);
  if (!engine) return null;
  return (
    <section aria-label="Docker timeline" className="flex flex-col gap-2">
      <ContainerHistory
        aggregate
        title="Container workload · CPU"
        metric="cpu_pct"
        page={history.data}
        from={from}
        to={to}
        events={events.data?.events}
      />
      <ContainerHistory
        aggregate
        title="Container workload · Working-set memory"
        metric="working_set_bytes"
        page={history.data}
        from={from}
        to={to}
        events={events.data?.events}
      />
      {events.error && (
        <p role="alert" className="text-[12px] text-muted-foreground">
          Container events unavailable: {events.error.message}
        </p>
      )}
      {events.data?.coverage.map((gap) => (
        <p
          key={`${gap.start_ms}-${gap.container_id}`}
          className="text-[11px] text-muted-foreground"
        >
          Event coverage gap: {gap.reason} ·{" "}
          {new Date(gap.start_ms).toLocaleString()}
        </p>
      ))}
      {history.error && (
        <p role="alert" className="text-[12px] text-muted-foreground">
          Container history unavailable: {history.error.message}
        </p>
      )}
      <div className="flex flex-wrap gap-x-4 gap-y-2 text-[11px]">
        {events.data?.events
          .filter((event) => event.container_id !== null)
          .map((event) => (
            <Link
              key={event.id}
              className="rounded-control text-link underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              to={`${containerPath(event.engine_id, event.container_id ?? "")}?at=${event.time_ms}`}
            >
              {event.name ?? event.container_id} · {event.action} ·{" "}
              {new Date(event.time_ms).toLocaleTimeString()}
            </Link>
          ))}
      </div>
    </section>
  );
}
export function DockerTimelineLane(props: { from: number; to: number }) {
  const local = useIsLocalHost();
  const enabled = useSettings((settings) => settings.docker?.enabled);
  return local && enabled ? <LiveTimelineLane {...props} /> : null;
}
