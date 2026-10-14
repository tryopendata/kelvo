import type { DockerContainer, DockerSnapshot } from "@core/generated/bindings";
import { dockerKeys } from "@core/query-keys";
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
import {
  ContainerActionButton,
  ContainerActionFailure,
  ContainerActionProvider,
} from "./_components/container-actions";
import { ContainerHistory } from "./_components/container-history";
import { ContainerLifecycle } from "./_components/container-lifecycle";
import { ContainerLogs } from "./_components/container-logs";
import { ContainerMetadata } from "./_components/container-metadata";
import { ContainerObservation } from "./_components/container-observation";
import { DockerLocalOnly } from "./_components/docker-local-only";
import {
  DOCKER_RANGES,
  type DockerRange,
  HistoryControls,
} from "./_components/history-controls";
import {
  useDocker,
  useDockerEvents,
  useDockerHistory,
} from "./_hooks/use-docker";
import { dockerRange, historyEnd } from "./_lib/history-params";
import { healthLabel, lifecycleLabel, statusLabel } from "./_lib/presentation";

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
  const [range, setRange] = useState<DockerRange>(() =>
    dockerRange(params.get("range"), "15m")
  );
  const [end, setEnd] = useState<number | null>(() =>
    params.has("end")
      ? historyEnd(params.get("end"))
      : historyEnd(params.get("at"), 60_000)
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
  const { live, row, current, connection } = containerObservation(
    snapshot,
    engineId,
    containerId,
    detail.isError ? undefined : detail.data
  );
  const now = Math.floor((snapshot?.now_ms ?? Date.now()) / 10_000) * 10_000;
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
  const back = location.state as {
    back?: string;
    scroll?: number;
    tableScroll?: number;
  } | null;
  return (
    <ContainerActionProvider snapshot={snapshot}>
      <div className="flex flex-col gap-4">
        <Button
          variant="ghost"
          size="sm"
          className="self-start"
          onClick={() =>
            navigate(back?.back ?? "/dashboard/containers", {
              state: {
                scroll: back?.scroll ?? 0,
                tableScroll: back?.tableScroll ?? 0,
              },
            })
          }
        >
          ← Containers
        </Button>
        <PageHeader
          title={row?.name ?? "Container"}
          subtitle={containerSubtitle(row)}
          actions={
            <div className="flex flex-wrap items-center gap-2">
              {live && <ContainerActionButton row={live} />}
              <HistoryControls
                range={range}
                end={end}
                onRange={setRange}
                onEnd={setEnd}
                now={now}
              />
            </div>
          }
        />
        {row && <ContainerActionFailure id={row.id} />}
        {detail.error && (
          <p role="alert" className="text-[12px] text-muted-foreground">
            Inspection unavailable: {detail.error.message}
          </p>
        )}
        {row && (
          <ContainerObservation
            row={row}
            current={current}
            connection={connection}
          />
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
        <ContainerLifecycle
          events={events}
          eventCursor={eventCursor}
          setEventCursor={setEventCursor}
        />
        {row && (
          <ContainerMetadata
            row={row}
            refreshing={detail.isFetching}
            onRefresh={() => void detail.refetch()}
          />
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
    </ContainerActionProvider>
  );
}

function containerObservation(
  snapshot: DockerSnapshot | null,
  engineId: string,
  containerId: string,
  inspected: DockerContainer | undefined
) {
  const live =
    snapshot?.engine?.id === engineId
      ? snapshot.containers.find((row) => row.id === containerId)
      : undefined;
  const row = live ?? inspected;
  const current = Boolean(
    live && snapshot?.status === "connected" && row?.observed_ms !== null
  );
  const connection =
    snapshot?.engine?.id === engineId
      ? statusLabel[snapshot.status]
      : "Retained engine · not selected";
  return { live, row, current, connection };
}

function containerSubtitle(row: DockerContainer | undefined) {
  return row
    ? `${row.image} · ${row.project ?? "Standalone"} · ${lifecycleLabel[row.lifecycle]} / ${healthLabel[row.health]} · last seen ${new Date(row.last_seen_ms).toLocaleString()}`
    : "Loading retained container metadata…";
}
