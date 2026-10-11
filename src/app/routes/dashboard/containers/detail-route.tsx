import { formatBytes, formatPercent } from "@core/format";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import {
  useLocation,
  useNavigate,
  useParams,
  useSearchParams,
} from "react-router";
import { PageHeader } from "~/components/page-header";
import { Button } from "~/components/ui/button";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";
import { ContainerHistory } from "./_components/container-history";
import { ContainerLogs } from "./_components/container-logs";
import { DockerLocalOnly } from "./_components/docker-local-only";
import {
  DOCKER_RANGES,
  type DockerRange,
  HistoryControls,
} from "./_components/history-controls";
import {
  dockerKeys,
  useDocker,
  useDockerEvents,
  useDockerHistory,
} from "./_hooks/use-docker";
import {
  healthLabel,
  lifecycleLabel,
  statusLabel,
  withInspection,
} from "./_lib/presentation";

export default function ContainerDetailRoute() {
  return (
    <DockerLocalOnly>
      <LocalContainerDetailRoute />
    </DockerLocalOnly>
  );
}

function LocalContainerDetailRoute() {
  const { engineId = "", containerId = "" } = useParams();
  const transport = useTransport();
  const host = useHostId();
  const navigate = useNavigate();
  const location = useLocation();
  const [params] = useSearchParams();
  const [range, setRange] = useState<DockerRange>("15m");
  const [end, setEnd] = useState<number | null>(() =>
    params.has("at") ? Number(params.get("at")) + 60_000 : null
  );
  const [logs, setLogs] = useState(false);
  const [eventCursor, setEventCursor] = useState<string | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: changing the investigation range restarts event pagination
  useEffect(() => setEventCursor(null), [engineId, containerId, range, end]);
  const { snapshot } = useDocker();
  const detail = useQuery({
    queryKey: dockerKeys.detail(
      host,
      engineId,
      containerId,
      snapshot?.generation ?? 0
    ),
    enabled: snapshot !== null,
    queryFn: () => unwrap(transport.dockerDetail(engineId, containerId)),
  });
  const live =
    snapshot?.engine?.id === engineId
      ? snapshot.containers.find((row) => row.id === containerId)
      : undefined;
  const inspected = detail.isError ? undefined : detail.data;
  const row = withInspection(live, inspected);
  const current = Boolean(
    live && snapshot?.status === "connected" && row?.observed_ms !== null
  );
  const connection =
    snapshot?.engine?.id === engineId
      ? statusLabel[snapshot.status]
      : "Retained engine · not selected";
  const now =
    Math.floor((snapshot?.observed_ms ?? Date.now()) / 10_000) * 10_000;
  const to = end ?? now;
  const from = to - DOCKER_RANGES[range];
  const history = useDockerHistory(
    engineId,
    containerId,
    from,
    to,
    snapshot?.generation
  );
  const events = useDockerEvents(
    engineId,
    containerId,
    from,
    to,
    snapshot?.generation,
    eventCursor
  );
  const back = location.state as { back?: string; scroll?: number } | null;
  return (
    <div className="flex flex-col gap-4">
      <Button
        variant="ghost"
        size="sm"
        className="self-start"
        onClick={() =>
          navigate(back?.back ?? "/dashboard/containers", {
            state: { scroll: back?.scroll ?? 0 },
          })
        }
      >
        ← Containers
      </Button>
      <PageHeader
        title={row?.name ?? "Container"}
        subtitle={
          row
            ? `${row.image} · ${row.project ?? "Standalone"} · ${lifecycleLabel[row.lifecycle]} / ${healthLabel[row.health]} · last seen ${new Date(row.last_seen_ms).toLocaleString()}`
            : "Loading retained container metadata…"
        }
        actions={
          <HistoryControls
            range={range}
            end={end}
            onRange={setRange}
            onEnd={setEnd}
            now={now}
          />
        }
      />
      {detail.error && (
        <p role="alert" className="text-[12px] text-muted-foreground">
          Inspection unavailable: {detail.error.message}
        </p>
      )}
      {row && (
        <>
          {row.lifecycle === "removed" && (
            <p role="status" className="text-[12px] text-muted-foreground">
              Removed container · retained history. Opening this page does not
              switch or reconnect the live engine.
            </p>
          )}
          <p role="status" className="text-[12px] text-muted-foreground">
            {connection} ·{" "}
            {current ? "Current observation" : "Retained observation"} ·{" "}
            {new Date(row.observed_ms ?? row.last_seen_ms).toLocaleString()}
          </p>
          <div className="figures flex flex-wrap gap-x-6 gap-y-2 text-[13px]">
            <span>
              {current ? "Now" : "Last observed"} · CPU{" "}
              {formatPercent(row.metrics.cpu_pct)}
            </span>
            <span>
              Working set {formatBytes(row.metrics.working_set_bytes)}
            </span>
            <span>
              Charged memory {formatBytes(row.metrics.raw_memory_bytes)} /{" "}
              {!row.metadata_available
                ? "Limit unavailable"
                : row.explicit_limit_bytes === null
                  ? "No explicit limit"
                  : formatBytes(row.explicit_limit_bytes)}
            </span>
          </div>
          <p className="text-[11px] text-muted-foreground">
            Charged memory is used to assess an explicit limit; the charts show
            cache-adjusted working set.
          </p>
          {(row.oom_killed || row.observed_restarts > 0) && (
            <p
              role="status"
              className="rounded-control border border-border px-3 py-2 text-[12px]"
            >
              {row.oom_killed ? "⚠ OOM recorded by Docker. " : ""}
              {row.repeated_restarts ? "Repeated restarts · " : ""}
              {row.observed_restarts} observed restarts in the last{" "}
              {row.observed_restart_window_ms / 60_000} minutes ·{" "}
              {row.restart_count ?? "Unknown"} cumulative Docker restarts. Exit
              code 137 alone does not establish OOM.
            </p>
          )}
        </>
      )}
      {history.error && (
        <p role="alert" className="text-[12px] text-muted-foreground">
          History unavailable: {history.error.message}
        </p>
      )}
      <ContainerHistory
        page={history.data}
        metric="cpu_pct"
        title="CPU · 100% is one core"
        from={from}
        to={to}
        events={events.data?.events}
      />
      <ContainerHistory
        page={history.data}
        metric="working_set_bytes"
        title="Working-set memory"
        from={from}
        to={to}
        events={events.data?.events}
      />
      <div className="grid min-w-0 gap-4 lg:grid-cols-2">
        <ContainerHistory
          page={history.data}
          metric="network_rx_bps"
          title="Network received"
          from={from}
          to={to}
        />
        <ContainerHistory
          page={history.data}
          metric="network_tx_bps"
          title="Network sent"
          from={from}
          to={to}
        />
        <ContainerHistory
          page={history.data}
          metric="block_read_bps"
          title="Block reads"
          from={from}
          to={to}
        />
        <ContainerHistory
          page={history.data}
          metric="block_write_bps"
          title="Block writes"
          from={from}
          to={to}
        />
      </div>
      <section aria-label="Lifecycle events" className="flex flex-col gap-2">
        <h2 className="text-[14px]">Lifecycle evidence</h2>
        {events.error && (
          <p role="alert" className="text-[12px]">
            Events unavailable: {events.error.message}
          </p>
        )}
        {events.data?.events.map((event) => (
          <div
            key={event.id}
            className="flex flex-wrap gap-3 border-border-subtle border-b py-2 text-[12px]"
          >
            <time
              className="figures text-muted-foreground"
              dateTime={new Date(event.time_ms).toISOString()}
            >
              {new Date(event.time_ms).toLocaleString()}
            </time>
            <span>
              {event.action}
              {event.exit_code !== null ? ` · exit ${event.exit_code}` : ""}
            </span>
            {event.action === "oom" && <span>Explicit Docker OOM event</span>}
          </div>
        ))}
        {events.data?.events.length === 0 && (
          <p className="text-[12px] text-muted-foreground">
            No recorded lifecycle events in this range.
          </p>
        )}
        <div className="flex gap-2">
          {eventCursor && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => setEventCursor(null)}
            >
              Latest events
            </Button>
          )}
          {events.data?.next_id && (
            <Button
              size="sm"
              variant="outline"
              onClick={() => setEventCursor(events.data?.next_id ?? null)}
            >
              Older events
            </Button>
          )}
        </div>
        {events.data?.coverage.map((gap) => (
          <p key={gap.start_ms} className="text-[11px] text-muted-foreground">
            Event coverage: {gap.reason}
          </p>
        ))}
      </section>
      {row && (
        <details className="border-border-subtle border-y py-3 text-[12px]">
          <summary className="cursor-pointer">Container metadata</summary>
          <Button
            size="sm"
            variant="outline"
            className="mt-2"
            disabled={detail.isFetching}
            onClick={() => void detail.refetch()}
          >
            {detail.isFetching ? "Refreshing metadata…" : "Refresh metadata"}
          </Button>
          {!row.metadata_available && (
            <p className="mt-2 text-muted-foreground">
              Inspection metadata unavailable. Inventory facts are shown below;
              an unknown limit or quota does not mean unrestricted.
            </p>
          )}
          <dl className="mt-3 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-6 gap-y-2">
            {[
              ["ID", row.id],
              ["Engine", row.engine_id],
              ["State", lifecycleLabel[row.lifecycle]],
              ["Created", row.created ?? "Unknown"],
              [
                "CPU quota",
                !row.metadata_available
                  ? "Unavailable"
                  : row.cpu_quota === null
                    ? "No explicit quota"
                    : String(row.cpu_quota),
              ],
              ["Restart policy", row.restart_policy ?? "Unknown"],
              [
                "Ports",
                !row.metadata_available
                  ? "Unavailable"
                  : row.ports.join(" · ") || "None",
              ],
              [
                "Mounts",
                !row.metadata_available
                  ? "Unavailable"
                  : row.mounts.join(" · ") || "None",
              ],
            ].map(([label, value]) => (
              <div key={label} className="contents">
                <dt className="text-muted-foreground">{label}</dt>
                <dd className="break-all">{value}</dd>
              </div>
            ))}
          </dl>
        </details>
      )}
      <Button
        variant="outline"
        size="sm"
        className="self-start"
        aria-expanded={logs}
        onClick={() => setLogs((value) => !value)}
      >
        {logs ? "Close logs" : "Open logs"}
      </Button>
      {logs && (
        <ContainerLogs
          engine={engineId}
          container={containerId}
          generation={snapshot?.generation ?? 0}
        />
      )}
    </div>
  );
}
