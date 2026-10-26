import { formatBytes, formatPercent } from "@core/format";
import { ChevronDown } from "lucide-react";
import { useLayoutEffect, useMemo, useRef } from "react";
import { Link } from "react-router";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { useTransport } from "~/lib/transport-context";
import type { PresentationProps } from "~/routes/popover/_components/popover-cards";
import { useSettings } from "~/stores/settings-store";
import { MeterTrack } from "~/widgets/meter-track";
import { ModuleCard } from "~/widgets/module-card";
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

function LivePopoverSummary({ compact = false, onToggle }: PresentationProps) {
  const toggle = useContainerDensityFocus(compact, onToggle);
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
  if (compact)
    return (
      <ModuleCard
        compact
        onToggle={toggle}
        accent="cpu"
        title="Containers"
        href="/dashboard/containers"
        onOpen={(route) => void transport.openDashboard(route)}
        value={`${snapshot.aggregate.running} running`}
        notice={
          [
            snapshot.status !== "connected"
              ? statusLabel[snapshot.status]
              : null,
            snapshot.aggregate.partial ? "Partial coverage" : null,
          ]
            .filter(Boolean)
            .join(" · ") || undefined
        }
      >
        <div className="flex items-center gap-2 text-[10px] text-muted-foreground">
          <span className="figures whitespace-nowrap">
            {snapshot.aggregate.cpu_cores?.toFixed(2) ?? "—"} cores ·{" "}
            {formatBytes(snapshot.aggregate.working_set_bytes)}
          </span>
          <MeterTrack
            fraction={
              snapshot.containers.length
                ? snapshot.aggregate.running / snapshot.containers.length
                : null
            }
            className="min-w-6 flex-1"
          />
        </div>
      </ModuleCard>
    );
  return (
    <section
      aria-label="Container glance"
      className="relative rounded-card border border-border bg-card-vibrant p-3"
    >
      <button
        type="button"
        onClick={() => void transport.openDashboard("/dashboard/containers")}
        className="block w-full rounded-control text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <span className="flex justify-between text-[12px]">
          <span className="pl-5">Containers</span>
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
      {toggle && (
        <button
          type="button"
          onClick={toggle}
          aria-label="Collapse Containers"
          aria-expanded
          className="absolute top-3 left-2 flex size-5 items-center justify-center rounded-control text-muted-foreground outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ChevronDown className="size-3.5" />
        </button>
      )}
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
/** Expanded Docker's action buttons use a different shell, so restore its toggle after swapping. */
function useContainerDensityFocus(
  compact: boolean,
  onToggle: (() => void) | undefined
) {
  const requested = useRef(false);
  useLayoutEffect(() => {
    if (!requested.current) return;
    document
      .querySelector<HTMLButtonElement>(
        `button[aria-label="${compact ? "Expand" : "Collapse"} Containers"]`
      )
      ?.focus({ preventScroll: true });
    requested.current = false;
  }, [compact]);
  return onToggle
    ? () => {
        requested.current = true;
        onToggle();
      }
    : undefined;
}

export function DockerPopoverSummary(props: PresentationProps) {
  const local = useIsLocalHost();
  const shown = useSettings(
    (settings) => settings.docker?.enabled && settings.docker?.show_popover
  );
  return local && shown ? <LivePopoverSummary {...props} /> : null;
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
