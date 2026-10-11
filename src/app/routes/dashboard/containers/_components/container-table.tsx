import { formatBytes, formatPercent, formatRate } from "@core/format";
import type { DockerContainer } from "@core/generated/bindings";
import { CheckIcon } from "lucide-react";
import { memo, useCallback, useLayoutEffect, useRef, useState } from "react";
import { Link, useLocation } from "react-router";
import { Button } from "~/components/ui/button";
import { useElementSize } from "~/hooks/use-element-size";
import {
  type ContainerSort,
  containerPath,
  healthLabel,
  lifecycleLabel,
  sortedContainers,
} from "../_lib/presentation";
import styles from "./container-table.module.css";

// Rank changes replace rows at the page boundary. Use a native control here
// to avoid mounting a Radix provider tree for each replacement, with the same
// visual tokens as the shared checkbox and native keyboard/checked semantics.
const ContainerMonitor = memo(function ContainerMonitor({
  id,
  name,
  checked,
  disabled,
  onChange,
}: {
  id: string;
  name: string;
  checked: boolean;
  disabled: boolean;
  onChange: (id: string, checked: boolean) => void;
}) {
  return (
    <span className="relative inline-flex size-4 align-middle">
      <input
        type="checkbox"
        aria-label={`Monitor ${name}`}
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(id, event.currentTarget.checked)}
        className="peer size-4 shrink-0 appearance-none rounded-chip border border-input outline-none transition-shadow checked:border-primary checked:bg-primary focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50 dark:bg-input/30 dark:checked:bg-primary"
      />
      <CheckIcon
        aria-hidden
        className="pointer-events-none absolute inset-0 m-auto hidden size-3.5 text-primary-foreground peer-checked:block peer-disabled:opacity-50"
      />
    </span>
  );
});

const ContainerIdentity = memo(
  function ContainerIdentity({
    row,
    monitoringFull,
    onMonitored,
    back,
    scroll,
  }: {
    row: DockerContainer;
    monitoringFull: boolean;
    onMonitored: (id: string, checked: boolean) => void;
    back: string;
    scroll: number;
  }) {
    return (
      <>
        <td className="min-w-0 px-3 py-1.5 font-normal">
          <span
            className="float-left mt-1 mr-2"
            title="Include this container in the monitored population"
          >
            <ContainerMonitor
              id={row.id}
              name={row.name}
              checked={row.monitored}
              disabled={!row.monitored && monitoringFull}
              onChange={onMonitored}
            />
          </span>
          <Link
            to={containerPath(row.engine_id, row.id)}
            state={{
              back,
              scroll,
            }}
            className="block truncate rounded-control text-[12px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
            title={`${row.name} · ${row.image}`}
          >
            {row.name}
          </Link>
          <div
            className="truncate text-[11px] text-muted-foreground"
            title={row.image}
          >
            {row.image}
          </div>
          <div className={styles.narrow}>
            {lifecycleLabel[row.lifecycle]} · {healthLabel[row.health]}
            <span className={styles.narrowMemory}>
              {" "}
              · {formatBytes(row.metrics.working_set_bytes)}
            </span>
          </div>
        </td>
        <td className={`${styles.state} px-3 py-1.5 font-normal text-[12px]`}>
          {lifecycleLabel[row.lifecycle]}
          <div className="text-[11px] text-muted-foreground">
            {healthLabel[row.health]}
          </div>
        </td>
      </>
    );
  },
  (a, b) =>
    a.back === b.back &&
    a.scroll === b.scroll &&
    a.monitoringFull === b.monitoringFull &&
    a.onMonitored === b.onMonitored &&
    a.row.id === b.row.id &&
    a.row.engine_id === b.row.engine_id &&
    a.row.name === b.row.name &&
    a.row.image === b.row.image &&
    a.row.lifecycle === b.row.lifecycle &&
    a.row.health === b.row.health &&
    a.row.monitored === b.row.monitored &&
    a.row.metrics.working_set_bytes === b.row.metrics.working_set_bytes
);

const ContainerMemory = memo(
  function ContainerMemory({ row, io }: { row: DockerContainer; io: boolean }) {
    return (
      <>
        <td
          className={`${styles.memory} figures px-3 py-1.5 text-right font-normal text-[12px]`}
        >
          {io
            ? formatRate(row.metrics.block_read_bps)
            : formatBytes(row.metrics.working_set_bytes)}
          <div className="text-[10px] text-muted-foreground">
            {io
              ? `write ${formatRate(row.metrics.block_write_bps)}`
              : !row.metadata_available
                ? "Limit unavailable"
                : row.explicit_limit_bytes === null
                  ? "No explicit limit"
                  : `/ ${formatBytes(row.explicit_limit_bytes)}`}
          </div>
        </td>
        <td
          className={`${styles.restarts} figures px-3 py-1.5 text-right font-normal text-[12px]`}
        >
          {row.restart_count ?? "—"}
        </td>
      </>
    );
  },
  (a, b) =>
    a.io === b.io &&
    a.row.metadata_available === b.row.metadata_available &&
    a.row.explicit_limit_bytes === b.row.explicit_limit_bytes &&
    a.row.restart_count === b.row.restart_count &&
    a.row.metrics.working_set_bytes === b.row.metrics.working_set_bytes &&
    a.row.metrics.block_read_bps === b.row.metrics.block_read_bps &&
    a.row.metrics.block_write_bps === b.row.metrics.block_write_bps
);

const ContainerUsage = memo(function ContainerUsage({
  primary,
  secondary,
}: {
  primary: string;
  secondary: string;
}) {
  return (
    <td className="figures whitespace-nowrap px-3 py-1.5 text-right font-normal text-[12px]">
      {primary}
      <div className="text-[10px] text-muted-foreground">{secondary}</div>
    </td>
  );
});

const ContainerGroupHeading = memo(function ContainerGroupHeading({
  group,
  count,
  columns,
  collapsed,
  onToggle,
}: {
  group: string;
  count: number;
  columns: number;
  collapsed: boolean;
  onToggle: (group: string) => void;
}) {
  return (
    <tr className="bg-raised/40">
      <th colSpan={columns} className="px-3 py-1 text-left">
        <button
          type="button"
          aria-expanded={!collapsed}
          onClick={() => onToggle(group)}
          className="flex w-full items-center gap-2 rounded-control py-1 text-[12px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <span aria-hidden>{collapsed ? "›" : "⌄"}</span>
          {group}
          <span className="font-normal text-muted-foreground">
            {count} containers
          </span>
        </button>
      </th>
    </tr>
  );
});

function ContainerTableImpl({
  rows,
  sort,
  onSort,
  grouped,
  io,
  collapsed,
  onCollapse,
  onMonitored,
  monitoringFull,
  page = 0,
  onPage,
}: {
  onMonitored: (id: string, monitored: boolean) => void;
  monitoringFull: boolean;
  page?: number;
  onPage?: (page: number) => void;
  rows: readonly DockerContainer[];
  sort: ContainerSort;
  onSort: (sort: ContainerSort) => void;
  grouped: boolean;
  io: boolean;
  collapsed: readonly string[];
  onCollapse: (group: string) => void;
}) {
  const location = useLocation();
  const monitorRef = useRef(onMonitored);
  const collapseRef = useRef(onCollapse);
  useLayoutEffect(() => {
    monitorRef.current = onMonitored;
    collapseRef.current = onCollapse;
  }, [onMonitored, onCollapse]);
  const toggleGroup = useCallback((group: string) => {
    collapseRef.current(group);
  }, []);
  const changeMonitored = useCallback((id: string, checked: boolean) => {
    monitorRef.current(id, checked);
  }, []);
  const [containerRef, width] = useElementSize("width", 1000);
  const columns = width <= 420 ? 2 : width <= 640 ? 3 : 5;
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [orderVersion, setOrderVersion] = useState(0);
  const held = useRef<{ version: number; ids: string[] }>({
    version: -1,
    ids: [],
  });
  const paused = hovered || focused;
  const sorted = sortedContainers(rows, sort);
  if (!paused || held.current.version !== orderVersion)
    held.current = { version: orderVersion, ids: sorted.map((row) => row.id) };
  const rank = new Map(held.current.ids.map((id, index) => [id, index]));
  const ordered = paused
    ? [...rows].sort(
        (a, b) =>
          (rank.get(a.id) ?? Infinity) - (rank.get(b.id) ?? Infinity) ||
          a.id.localeCompare(b.id)
      )
    : sorted;
  const pageCount = Math.max(1, Math.ceil(ordered.length / 32));
  const currentPage = Math.min(Math.max(0, page), pageCount - 1);
  const pageRows = ordered.slice(currentPage * 32, (currentPage + 1) * 32);
  const scroll = document.querySelector("main")?.scrollTop ?? 0;
  const groups = grouped
    ? [...new Set(pageRows.map((row) => row.project ?? "Standalone"))]
    : [""];
  const items = groups.flatMap((group) => {
    const members = grouped
      ? pageRows.filter((row) => (row.project ?? "Standalone") === group)
      : pageRows;
    return [
      ...(grouped
        ? [
            <ContainerGroupHeading
              key={`group:${group}`}
              group={group}
              count={members.length}
              columns={columns}
              collapsed={collapsed.includes(group)}
              onToggle={toggleGroup}
            />,
          ]
        : []),
      ...(!grouped || !collapsed.includes(group)
        ? members.map((row) => (
            <tr
              key={row.id}
              className="border-border-subtle border-t hover:bg-selected/50"
            >
              <ContainerIdentity
                row={row}
                monitoringFull={monitoringFull}
                onMonitored={changeMonitored}
                back={`${location.pathname}${location.search}`}
                scroll={scroll}
              />
              <ContainerUsage
                primary={
                  io
                    ? formatRate(row.metrics.network_rx_bps)
                    : formatPercent(row.metrics.cpu_pct)
                }
                secondary={
                  io
                    ? `↑ ${formatRate(row.metrics.network_tx_bps)}`
                    : !row.monitored
                      ? "Not monitored"
                      : ""
                }
              />
              <ContainerMemory row={row} io={io} />
            </tr>
          ))
        : []),
    ];
  });
  const heading = (label: string, key: ContainerSort) => (
    <button
      type="button"
      className="rounded-control focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      onClick={() => {
        onSort(key);
        setOrderVersion((v) => v + 1);
      }}
    >
      {label}
      {sort === key ? (key === "name" ? " ↑" : " ↓") : ""}
    </button>
  );
  return (
    <div ref={containerRef} className={styles.container}>
      <div className="flex h-9 items-center justify-between px-3 text-[11px] text-muted-foreground">
        <span>Now · {rows.length} shown</span>
        {pageCount > 1 && (
          <span className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={currentPage === 0}
              onClick={() => onPage?.(currentPage - 1)}
            >
              Previous rows
            </Button>
            <span className="figures">
              {currentPage * 32 + 1}–
              {Math.min(rows.length, (currentPage + 1) * 32)}
            </span>
            <Button
              variant="ghost"
              size="sm"
              disabled={currentPage + 1 >= pageCount}
              onClick={() => onPage?.(currentPage + 1)}
            >
              Next rows
            </Button>
          </span>
        )}
        <span className="flex items-center gap-2" role="status">
          {paused && (
            <>
              Order paused
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setOrderVersion((v) => v + 1)}
              >
                Reorder now
              </Button>
            </>
          )}
        </span>
      </div>
      <div
        onPointerEnter={() => setHovered(true)}
        onPointerLeave={() => setHovered(false)}
        onFocusCapture={() => setFocused(true)}
        onBlurCapture={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget)) setFocused(false);
        }}
      >
        <table
          className="w-full table-fixed border-collapse"
          aria-label="Containers now"
        >
          <thead className="border-border border-y text-[11px] text-muted-foreground">
            <tr>
              <th
                className="w-[32%] px-3 py-2 text-left font-normal"
                aria-sort={sort === "name" ? "ascending" : "none"}
              >
                {heading("Name / image", "name")}
              </th>
              <th className={`${styles.state} px-3 py-2 text-left font-normal`}>
                State / health
              </th>
              <th
                className="px-3 py-2 text-right font-normal"
                aria-sort={sort === "cpu" ? "descending" : "none"}
              >
                {io ? "Network ↓ / ↑" : heading("CPU", "cpu")}
              </th>
              <th
                className={`${styles.memory} px-3 py-2 text-right font-normal`}
                aria-sort={sort === "memory" ? "descending" : "none"}
              >
                {io ? "Block I/O" : heading("Memory / limit", "memory")}
              </th>
              <th
                className={`${styles.restarts} w-20 px-3 py-2 text-right font-normal`}
                aria-sort={sort === "restarts" ? "descending" : "none"}
              >
                {heading("Restarts", "restarts")}
              </th>
            </tr>
          </thead>
          <tbody>{items}</tbody>
        </table>
      </div>
      {rows.length === 0 && (
        <p className="px-3 py-8 text-[12px] text-muted-foreground">
          No containers match this view.
        </p>
      )}
    </div>
  );
}

/** Callback references change with URL mirrors; only the view inputs draw rows. */
export const ContainerTable = memo(
  ContainerTableImpl,
  (a, b) =>
    a.page === b.page &&
    a.rows === b.rows &&
    a.sort === b.sort &&
    a.grouped === b.grouped &&
    a.io === b.io &&
    a.monitoringFull === b.monitoringFull &&
    a.onMonitored === b.onMonitored &&
    a.collapsed.join("\0") === b.collapsed.join("\0")
);
