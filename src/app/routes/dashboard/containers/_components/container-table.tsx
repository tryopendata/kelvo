import { seriesColor } from "@core/chart-math/series-lines";
import { formatBytes, formatPercent, formatRate } from "@core/format";
import type { DockerContainer } from "@core/generated/bindings";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  type MouseEvent,
  memo,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { Link, useLocation, useNavigate } from "react-router";
import { SeriesSwatch } from "~/components/charts/chart-tooltip";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuTrigger,
} from "~/components/ui/context-menu";
import { useUnits } from "~/hooks/use-units";
import { cn } from "~/lib/utils";
import {
  containerReturnState,
  isContainerDetailClick,
} from "../_lib/navigation";
import {
  type ContainerSort,
  containerPath,
  healthLabel,
  lifecycleLabel,
  sortedContainers,
} from "../_lib/presentation";
import {
  ContainerActionButton,
  ContainerActionMenuItem,
  containerAction,
  useContainerActions,
} from "./container-actions";
import styles from "./container-table.module.css";

export const needsAttention = (row: DockerContainer) =>
  row.health === "unhealthy" || row.oom_killed || row.repeated_restarts;
export function ContainerTable({
  rows,
  sort,
  onSort,
  grouped,
  collapsed = [],
  onCollapse,
  slots = {},
  focused = null,
  onFocus,
  columns = [],
  filters,
  attention,
  historical = false,
  title = "Containers now",
  liveRows,
}: {
  rows: readonly DockerContainer[];
  sort: ContainerSort;
  onSort: (sort: ContainerSort) => void;
  grouped: boolean;
  collapsed?: readonly string[];
  onCollapse: (group: string) => void;
  slots?: Record<string, number>;
  focused?: string | null;
  onFocus?: (id: string | null) => void;
  columns?: readonly string[];
  filters?: ReactNode;
  attention?: ReactNode;
  historical?: boolean;
  title?: string;
  liveRows?: readonly DockerContainer[];
}) {
  const location = useLocation();
  const navigate = useNavigate();
  const actions = useContainerActions();
  const { bytes, rate } = useUnits();
  const scroller = useRef<HTMLDivElement>(null);
  const [hovered, setHovered] = useState(false);
  const [keyboard, setKeyboard] = useState(false);
  const [contextId, setContextId] = useState<string | null>(null);
  const [contextOpen, setContextOpen] = useState(false);
  const contextRow = (liveRows ?? (historical ? [] : rows)).find(
    (row) => row.id === contextId
  );
  const held = useRef<string[]>([]);
  const previousSort = useRef(sort);
  const paused =
    hovered || keyboard || contextOpen || actions.pendingIds.size > 0;
  const sorted = sortedContainers(rows, sort);
  if (!paused || previousSort.current !== sort)
    held.current = sorted.map((row) => row.id);
  previousSort.current = sort;
  const rank = new Map(held.current.map((id, index) => [id, index]));
  const ordered = paused
    ? [...rows].sort(
        (a, b) =>
          (rank.get(a.id) ?? Infinity) - (rank.get(b.id) ?? Infinity) ||
          a.id.localeCompare(b.id)
      )
    : sorted;
  const groups = grouped
    ? [...new Set(ordered.map((row) => row.project ?? "Standalone"))]
    : [""];
  const items: (
    | { kind: "group"; group: string; count: number }
    | { kind: "row"; row: DockerContainer }
  )[] = groups.flatMap((group) => {
    const members = grouped
      ? ordered.filter((row) => (row.project ?? "Standalone") === group)
      : ordered;
    return [
      ...(grouped
        ? [{ kind: "group" as const, group, count: members.length }]
        : []),
      ...(!grouped || !collapsed.includes(group)
        ? members.map((row) => ({ kind: "row" as const, row }))
        : []),
    ];
  });
  const virtual = useVirtualizer({
    count: items.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 44,
    overscan: 5,
    enabled: rows.length > 100,
    getItemKey: (index) => {
      const item = items[index];
      return !item
        ? index
        : item.kind === "group"
          ? `group:${item.group}`
          : item.row.id;
    },
  });
  useEffect(() => {
    const saved =
      (location.state as { tableScroll?: number } | null)?.tableScroll ??
      window.history.state?.containerTableScroll;
    if (typeof saved === "number" && scroller.current)
      scroller.current.scrollTop = saved;
  }, [location.state]);
  useEffect(() => {
    if (!focused || hovered) return;
    const index = items.findIndex(
      (item) => item.kind === "row" && item.row.id === focused
    );
    if (index < 0) return;
    if (rows.length > 100) virtual.scrollToIndex(index, { align: "auto" });
    else {
      const row = scroller.current?.querySelector(
        `[data-container-id="${CSS.escape(focused)}"]`
      );
      if (row && scroller.current) {
        const bounds = row.getBoundingClientRect();
        const view = scroller.current.getBoundingClientRect();
        if (bounds.top < view.top + 32 || bounds.bottom > view.bottom)
          row.scrollIntoView({ block: "nearest" });
      }
    }
  }, [focused, hovered, items, rows.length, virtual]);
  const count = 7 + columns.length;
  const heading = (label: string, key: ContainerSort) => (
    <button
      type="button"
      onClick={() => onSort(key)}
      className="rounded-control focus-visible:ring-2 focus-visible:ring-ring"
    >
      {label}
      {sort === key ? (key === "name" ? " ↑" : " ↓") : ""}
    </button>
  );
  const openDetail = useCallback(
    (event: MouseEvent<HTMLAnchorElement>, engine: string, id: string) => {
      if (!isContainerDetailClick(event)) return;
      event.preventDefault();
      void navigate(containerPath(engine, id), {
        state: containerReturnState(location, scroller.current),
      });
    },
    [navigate, location]
  );
  const byte = (value: number | null) => formatBytes(value, { units: bytes });
  const render = (item: (typeof items)[number]) => {
    if (item.kind === "group")
      return (
        <tr key={`group:${item.group}`} className="h-11 bg-raised/40">
          <th colSpan={count} className="px-3 text-left">
            <button
              className="w-full text-left text-[12px]"
              type="button"
              aria-expanded={!collapsed.includes(item.group)}
              onClick={() => onCollapse(item.group)}
            >
              {collapsed.includes(item.group) ? "›" : "⌄"} {item.group}
              <span className="ml-2 font-normal text-muted-foreground">
                {item.count} containers
              </span>
            </button>
          </th>
        </tr>
      );
    const row = item.row;
    const current =
      liveRows?.find((r) => r.id === row.id) ?? (historical ? null : row);
    const live = ["running", "paused", "restarting"].includes(row.lifecycle);
    const metrics = historical || live ? row.metrics : null;
    const health = [
      row.health !== "no_check" && row.health !== "healthy"
        ? healthLabel[row.health]
        : null,
      row.oom_killed ? "OOM-killed" : null,
      row.repeated_restarts ? `${row.observed_restarts} restarts` : null,
    ]
      .filter(Boolean)
      .join(" · ");
    return (
      <tr
        key={row.id}
        data-container-id={row.id}
        onPointerEnter={() => onFocus?.(slots[row.id] ? row.id : null)}
        onPointerLeave={() => onFocus?.(null)}
        onFocus={() => onFocus?.(slots[row.id] ? row.id : null)}
        className={cn(
          "group h-11 border-border-subtle border-t hover:bg-selected/50",
          focused === row.id && "bg-selected"
        )}
      >
        <td className="w-5 pl-3">
          {slots[row.id] && (
            <SeriesSwatch color={seriesColor(slots[row.id] ?? 1)} />
          )}
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
          failure={actions.failures[row.id]}
          onNavigate={openDetail}
        />
        <td className="figures whitespace-nowrap px-2 py-1 text-right text-[12px]">
          {formatPercent(metrics?.cpu_pct ?? null)}
        </td>
        <td
          className={`${styles.memory} figures px-2 py-1 text-right text-[12px] leading-[14px]`}
        >
          <span className="whitespace-nowrap">
            {byte(metrics?.working_set_bytes ?? null)}
          </span>
          {metrics && row.explicit_limit_bytes !== null && !historical && (
            <>
              {" "}
              <span className="whitespace-nowrap">
                / {byte(row.explicit_limit_bytes)}
              </span>
            </>
          )}
          {metrics && row.explicit_limit_bytes !== null && !historical && (
            <div className="mt-1 ml-auto h-1 w-12 overflow-hidden rounded-mark bg-track">
              <div
                className="h-full origin-left bg-muted-foreground"
                style={{
                  transform: `scaleX(${Math.min(1, (metrics.working_set_bytes ?? 0) / Math.max(1, row.explicit_limit_bytes))})`,
                }}
              />
            </div>
          )}
        </td>
        <td
          className={`${styles.net} figures whitespace-nowrap px-2 py-1 text-right text-[12px]`}
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
            {column === "io"
              ? `${formatRate(metrics?.block_read_bps ?? null)} / ${formatRate(metrics?.block_write_bps ?? null)}`
              : column === "image"
                ? row.image
                : column === "ports"
                  ? row.ports.join(", ") || "—"
                  : column === "restarts"
                    ? (row.restart_count ?? "—")
                    : "—"}
          </td>
        ))}
        <td className="w-18 px-2 text-right">
          {current && <ContainerActionButton row={current} reveal />}
        </td>
      </tr>
    );
  };
  const virtualItems = virtual.getVirtualItems();
  const top = virtualItems[0]?.start ?? 0;
  const bottom = virtual.getTotalSize() - (virtualItems.at(-1)?.end ?? 0);
  return (
    <div className={styles.container}>
      <div className="flex flex-wrap items-center gap-3 px-3 py-2">
        <p className="mr-auto text-[14px]">{title}</p>
        {filters}
        <span className="text-[11px] text-muted-foreground">
          {rows.length} matching containers
        </span>
        {attention}
      </div>
      <ContextMenu onOpenChange={setContextOpen}>
        <ContextMenuTrigger asChild>
          <div
            onContextMenuCapture={(event) => {
              const target =
                event.target instanceof Element
                  ? event.target.closest<HTMLElement>("tr[data-container-id]")
                  : null;
              const row = (liveRows ?? (historical ? [] : rows)).find(
                (candidate) => candidate.id === target?.dataset.containerId
              );
              if (!row || !containerAction(row)) {
                event.preventDefault();
                setContextId(null);
                return;
              }
              setContextId(row.id);
            }}
            data-container-table-scroll
            ref={scroller}
            className="max-h-88 overflow-y-scroll"
            onPointerEnter={() => setHovered(true)}
            onPointerLeave={() => {
              setHovered(false);
              onFocus?.(null);
            }}
            onFocusCapture={() => setKeyboard(true)}
            onBlurCapture={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget)) {
                setKeyboard(false);
                onFocus?.(null);
              }
            }}
          >
            <table
              className="w-full table-fixed border-collapse"
              aria-label={title}
            >
              <thead className="sticky top-0 z-10 bg-card text-[11px] text-muted-foreground">
                <tr className="h-8 border-border border-y">
                  <th className="w-5">
                    <span className="sr-only">Chart color</span>
                  </th>
                  <th
                    className={`${styles.identity} px-2 text-left font-normal`}
                    aria-sort={sort === "name" ? "ascending" : "none"}
                  >
                    {heading("Name / project", "name")}
                  </th>
                  <th className={`${styles.state} px-2 text-left font-normal`}>
                    State
                  </th>
                  <th
                    className={`${styles.cpu} px-2 text-right font-normal`}
                    aria-sort={sort === "cpu" ? "descending" : "none"}
                  >
                    {heading(historical ? "Peak CPU" : "CPU", "cpu")}
                  </th>
                  <th
                    className={`${styles.memory} px-2 text-right font-normal`}
                    aria-sort={sort === "memory" ? "descending" : "none"}
                  >
                    {heading(historical ? "Peak memory" : "Memory", "memory")}
                  </th>
                  <th
                    className={`${styles.net} px-2 text-right font-normal`}
                    hidden={historical}
                  >
                    {heading("Net", "network")}
                  </th>
                  {columns.map((column) => (
                    <th key={column} className="px-2 text-right font-normal">
                      {column === "io"
                        ? heading("Block I/O", "io")
                        : column === "restarts"
                          ? heading("Restarts", "restarts")
                          : column === "image"
                            ? "Image"
                            : column === "ports"
                              ? "Ports"
                              : "PIDs"}
                    </th>
                  ))}
                  <th className="w-18">
                    <span className="sr-only">Action</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.length > 100 ? (
                  <>
                    {top > 0 && (
                      <tr aria-hidden>
                        <td colSpan={count} style={{ height: top }} />
                      </tr>
                    )}
                    {virtualItems.map((item) => {
                      const entry = items[item.index];
                      return entry ? render(entry) : null;
                    })}
                    {bottom > 0 && (
                      <tr aria-hidden>
                        <td colSpan={count} style={{ height: bottom }} />
                      </tr>
                    )}
                  </>
                ) : (
                  items.map(render)
                )}
              </tbody>
            </table>
            {!rows.length && (
              <p className="px-3 py-8 text-[12px] text-muted-foreground">
                No containers match this view.
              </p>
            )}
          </div>
        </ContextMenuTrigger>
        {contextRow && (
          <ContextMenuContent>
            <ContainerActionMenuItem row={contextRow} />
          </ContextMenuContent>
        )}
      </ContextMenu>
    </div>
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
          className={styles.narrowState}
          title={`${lifecycleLabel[lifecycle]} · ${health}`}
        >
          {lifecycleLabel[lifecycle]}
          {health ? ` · ${health}` : ""}
        </span>
      </td>
      <td className={`${styles.state} px-2 py-1 text-[12px]`}>
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
