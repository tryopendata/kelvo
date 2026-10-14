import { seriesColor } from "@core/chart-math/series-lines";
import { formatBytes, formatPercent, formatRate } from "@core/format";
import type {
  DockerContainer,
  DockerRankedContainer,
} from "@core/generated/bindings";
import { type MouseEvent, memo, type ReactNode } from "react";
import { Link } from "react-router";
import { SeriesSwatch } from "~/components/charts/chart-tooltip";
import type { useUnits } from "~/hooks/use-units";
import { cn } from "~/lib/utils";
import {
  containerPath,
  healthLabel,
  lifecycleLabel,
} from "../_lib/presentation";
import { ContainerActionButton } from "./container-actions";

export function ContainerDataRow({
  row,
  summary,
  current,
  slot,
  isFocused,
  onFocus,
  failure,
  openDetail,
  historical,
  columns,
  bytes,
  rate,
}: {
  row: DockerContainer;
  summary: DockerRankedContainer | undefined;
  current: DockerContainer | null | undefined;
  slot: number | undefined;
  isFocused: boolean;
  onFocus: ((id: string | null) => void) | undefined;
  failure: string | undefined;
  openDetail: (
    event: MouseEvent<HTMLAnchorElement>,
    engine: string,
    id: string
  ) => void;
  historical: boolean;
  columns: readonly string[];
  bytes: ReturnType<typeof useUnits>["bytes"];
  rate: ReturnType<typeof useUnits>["rate"];
}) {
  const byte = (value: number | null) => formatBytes(value, { units: bytes });
  const live = ["running", "paused", "restarting"].includes(row.lifecycle);
  const metrics = historical || live ? row.metrics : null;
  const health = containerHealth(row);

  return (
    <tr
      key={row.id}
      data-container-id={row.id}
      onPointerEnter={() => onFocus?.(slot ? row.id : null)}
      onPointerLeave={() => onFocus?.(null)}
      onFocus={() => onFocus?.(slot ? row.id : null)}
      className={cn(
        "group h-11 border-border-subtle border-t hover:bg-selected/50",
        isFocused && "bg-selected"
      )}
    >
      <td className="w-5 pl-3">
        {slot && <SeriesSwatch color={seriesColor(slot ?? 1)} />}
      </td>
      <ContainerIdentityCells
        engineId={row.engine_id}
        id={row.id}
        name={row.name}
        image={row.image}
        project={row.project}
        lifecycle={row.lifecycle}
        metadataObservedMs={row.metadata_observed_ms}
        health={health}
        failure={failure}
        onNavigate={openDetail}
      />
      <td className="figures whitespace-nowrap px-2 py-1 text-right text-[12px]">
        {formatPercent(
          metrics?.cpu_pct ?? null,
          historical ? { significantDigits: 3 } : {}
        )}
        {summary && (
          <div className="text-[10px] text-muted-foreground">
            {formatPercent(summary.avg_cpu_pct, { significantDigits: 3 })} avg
          </div>
        )}
      </td>
      <ContainerMemoryCell
        row={row}
        summary={summary}
        metrics={metrics}
        historical={historical}
        byte={byte}
      />
      <td
        className={`figures whitespace-nowrap px-2 py-1 text-right text-[12px] [@container(max-width:719px)]:hidden`}
        hidden={historical}
      >
        {formatRate(
          metrics?.network_rx_bps != null && metrics.network_tx_bps !== null
            ? metrics.network_rx_bps + metrics.network_tx_bps
            : null,
          { units: rate }
        )}
      </td>
      {columns.map((column) => (
        <td
          key={column}
          className="figures max-w-32 truncate px-2 text-right text-[11px]"
        >
          {extraColumnValue(column, row, metrics)}
        </td>
      ))}
      <td className="w-18 px-2 text-right">
        {current && <ContainerActionButton row={current} reveal />}
      </td>
    </tr>
  );
}
// Sampling changes metrics every second; identity and lifecycle change independently.
const ContainerIdentityCells = memo(function ContainerIdentityCells({
  engineId,
  id,
  name,
  image,
  project,
  lifecycle,
  metadataObservedMs,
  health,
  failure,
  onNavigate,
}: {
  engineId: string;
  id: string;
  name: string;
  image: string;
  project: string | null;
  lifecycle: DockerContainer["lifecycle"];
  metadataObservedMs: DockerContainer["metadata_observed_ms"];
  health: string;
  failure: string | undefined;
  onNavigate: (
    event: MouseEvent<HTMLAnchorElement>,
    engine: string,
    id: string
  ) => void;
}) {
  return (
    <>
      <td className="min-w-0 px-2 py-1 text-[12px]">
        <Link
          to={containerPath(engineId, id)}
          onClick={(event) => onNavigate(event, engineId, id)}
          className="block truncate rounded-control focus-visible:ring-2 focus-visible:ring-ring"
          title={`${name} · ${image}`}
        >
          {name}
          <span className="text-muted-foreground">
            {project ? ` · ${project}` : ""}
          </span>
        </Link>
        {failure && (
          <p role="alert" className="text-[11px] text-destructive">
            {failure}
          </p>
        )}
        <span
          className="hidden truncate text-[10px] text-muted-foreground [@container(max-width:480px)]:block"
          title={`${lifecycleLabel[lifecycle]} · ${health}`}
        >
          {lifecycleLabel[lifecycle]}
          {health ? ` · ${health}` : ""}
        </span>
      </td>
      <td
        className={`px-2 py-1 text-[12px] [@container(max-width:480px)]:hidden`}
      >
        {lifecycleLabel[lifecycle]}
        {health && (
          <div
            className="truncate text-[10px] text-muted-foreground"
            title={`${health} · ${metadataObservedMs ? new Date(metadataObservedMs).toLocaleString() : "time unknown"}`}
          >
            {health} ·{" "}
            {metadataObservedMs
              ? new Date(metadataObservedMs).toLocaleTimeString([], {
                  hour: "2-digit",
                  minute: "2-digit",
                })
              : "time unknown"}
          </div>
        )}
      </td>
    </>
  );
});

function containerHealth(row: DockerContainer): string {
  return [
    row.health !== "no_check" && row.health !== "healthy"
      ? healthLabel[row.health]
      : null,
    row.oom_killed ? "OOM-killed" : null,
    row.repeated_restarts ? `${row.observed_restarts} restarts` : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

function extraColumnValue(
  column: string,
  row: DockerContainer,
  metrics: DockerContainer["metrics"] | null
): ReactNode {
  switch (column) {
    case "io":
      return `${formatRate(metrics?.block_read_bps ?? null)} / ${formatRate(metrics?.block_write_bps ?? null)}`;
    case "image":
      return row.image;
    case "ports":
      return row.ports.join(", ") || "—";
    case "restarts":
      return row.restart_count ?? "—";
    default:
      return "—";
  }
}

function ContainerMemoryCell({
  row,
  summary,
  metrics,
  historical,
  byte,
}: {
  row: DockerContainer;
  summary: DockerRankedContainer | undefined;
  metrics: DockerContainer["metrics"] | null;
  historical: boolean;
  byte: (value: number | null) => string;
}) {
  const limit = row.explicit_limit_bytes;
  const showLimit = metrics !== null && limit !== null && !historical;
  return (
    <td
      className={`figures px-2 py-1 text-right text-[12px] leading-[14px] [@container(max-width:480px)]:w-[92px]`}
    >
      <span className="whitespace-nowrap">
        {byte(metrics?.working_set_bytes ?? null)}
      </span>
      {summary && (
        <div className="whitespace-nowrap text-[10px] text-muted-foreground">
          {byte(summary.avg_working_set_bytes)} avg
        </div>
      )}
      {showLimit && (
        <>
          {" "}
          <span className="whitespace-nowrap">/ {byte(limit)}</span>
        </>
      )}
      {showLimit && (
        <div className="mt-1 ml-auto h-1 w-12 overflow-hidden rounded-mark bg-track">
          <div
            className="h-full origin-left bg-muted-foreground"
            style={{
              transform: `scaleX(${Math.min(1, (metrics.working_set_bytes ?? 0) / Math.max(1, limit))})`,
            }}
          />
        </div>
      )}
    </td>
  );
}
