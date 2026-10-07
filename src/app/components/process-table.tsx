import {
  type ByteUnits,
  fixed,
  formatBytes,
  formatRate,
  MISSING,
  type RateUnits,
} from "@core/format";
import type { LiveProcess } from "@core/generated/bindings";
import { observeElementRect, useVirtualizer } from "@tanstack/react-virtual";
import { type ReactNode, useMemo, useRef, useState } from "react";
import { ContextMenu, ContextMenuTrigger } from "~/components/ui/context-menu";
import { cn } from "~/lib/utils";
import { InitialChip } from "~/widgets/initial-chip";

export type ProcessColumn =
  | "name"
  | "pid"
  | "cpu"
  | "mem"
  | "compressed"
  | "threads"
  | "wakeups"
  | "energy"
  | "diskRead"
  | "diskWrite"
  | "diskTotal"
  | "netRx"
  | "netTx"
  | "netTotal"
  | "gpu"
  | "user";

export interface ProcessSort {
  by: ProcessColumn;
  dir: "asc" | "desc";
}

export interface ProcessTableProps {
  rows: readonly LiveProcess[];
  columns: readonly ProcessColumn[];
  sort: ProcessSort;
  onSort: (sort: ProcessSort) => void;
  /** % CPU that fills the inline bar. The CPU page scales to 140%. */
  cpuBarMaxPct?: number;
  /** Viewport height of the scrolling body, px. Rows past it are virtualized. */
  height: number;
  memUnits?: ByteUnits;
  rateUnits?: RateUnits;
  /** Row click, for selection and row actions on the Processes page. */
  onSelect?: (row: LiveProcess) => void;
  /**
   * Controls in a trailing cell, shown while the row is hovered or has focus
   * (the Processes page's Quit and Force Quit).
   */
  rowAction?: (row: LiveProcess) => ReactNode;
  /** A `<ContextMenuContent>` for the row's context menu. */
  rowContextMenu?: (row: LiveProcess) => ReactNode;
  /**
   * Keep row order while the pointer is over the table so a row does not move
   * under the cursor; values keep updating, new rows go last, and order
   * resumes when the pointer leaves (plan 4.14). Sorting re-orders at once.
   */
  freezeOrderOnHover?: boolean;
  /**
   * Size the box to the most rows seen while mounted, up to `height`, rather
   * than to the rows now: a table whose rows come and go at 1 Hz (idle
   * processes dropping out) grows but never shrinks. The mark lives as long
   * as the table, so a caller keeps the table mounted through empty batches
   * and passes `empty` rather than swapping in its own message.
   */
  growOnly?: boolean;
  /** One row's message while there are no rows ("Measuring…"). */
  empty?: ReactNode;
  ariaLabel: string;
}

/** Row height in px: 12 px text plus 7 px padding top and bottom, plus a hairline. */
const ROW_H = 29;

const LABELS: Record<ProcessColumn, string> = {
  name: "Process",
  pid: "PID",
  cpu: "% CPU",
  mem: "Memory",
  compressed: "Compressed",
  threads: "Threads",
  wakeups: "Idle wake-ups",
  energy: "Energy",
  diskRead: "Disk read",
  diskWrite: "Disk write",
  diskTotal: "Disk total",
  netRx: "Down",
  netTx: "Up",
  netTotal: "Net total",
  gpu: "% GPU",
  user: "User",
};

const TEXT_COLUMNS: ReadonlySet<ProcessColumn> = new Set(["name", "user"]);

function sortValue(p: LiveProcess, by: ProcessColumn): number | string | null {
  switch (by) {
    case "name":
      return p.name.toLowerCase();
    case "user":
      return p.user;
    case "pid":
      return p.pid;
    case "cpu":
      return p.cpu_pct;
    case "mem":
      return p.mem_bytes;
    case "compressed":
      return p.compressed_bytes;
    case "threads":
      return p.threads;
    case "wakeups":
      return p.idle_wakeups_per_s;
    case "energy":
      return p.energy;
    case "diskRead":
      return p.disk_read_bps;
    case "diskWrite":
      return p.disk_write_bps;
    case "diskTotal":
      return diskTotal(p);
    case "netRx":
      return p.net_rx_bps;
    case "netTx":
      return p.net_tx_bps;
    case "netTotal":
      return netTotal(p);
    case "gpu":
      return p.gpu_pct;
  }
}

/**
 * Read plus write, bytes per second, as Rust ranks `disk_total`
 * (`procview.rs`); null when either is not a number.
 */
export function diskTotal(p: LiveProcess): number | null {
  if (p.disk_read_bps == null || p.disk_write_bps == null) return null;
  return p.disk_read_bps + p.disk_write_bps;
}

/**
 * Received plus sent, bytes per second; null when the row carries no
 * network rates (not sampled, D-081).
 */
export function netTotal(p: LiveProcess): number | null {
  if (p.net_rx_bps == null || p.net_tx_bps == null) return null;
  return p.net_rx_bps + p.net_tx_bps;
}

/**
 * `sorted` in the order of `keys` (the order when the pointer entered), with
 * rows that have since appeared appended in sorted order and rows that have
 * gone dropped.
 */
export function frozenOrder(
  sorted: readonly LiveProcess[],
  keys: readonly string[]
): LiveProcess[] {
  const byKey = new Map(sorted.map((p) => [processKey(p), p]));
  const out: LiveProcess[] = [];
  const placed = new Set<string>();
  for (const k of keys) {
    const p = byKey.get(k);
    if (p) {
      out.push(p);
      placed.add(k);
    }
  }
  for (const p of sorted) if (!placed.has(processKey(p))) out.push(p);
  return out;
}

/** Stable identity: a pid is reused, (pid, start time) is not. */
export function processKey(p: LiveProcess): string {
  return `${p.pid}:${p.start_time_us}`;
}

/** Sort a copy; `null` values go last in either direction, ties by pid. */
export function sortProcesses(
  rows: readonly LiveProcess[],
  { by, dir }: ProcessSort
): LiveProcess[] {
  const sign = dir === "asc" ? 1 : -1;
  return [...rows].sort((a, b) => {
    const va = sortValue(a, by);
    const vb = sortValue(b, by);
    if (va == null && vb == null) return a.pid - b.pid;
    if (va == null) return 1;
    if (vb == null) return -1;
    if (va < vb) return -sign;
    if (va > vb) return sign;
    return a.pid - b.pid;
  });
}

const grouped = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });

function num(v: number | null, decimals = 0): string {
  if (v == null || !Number.isFinite(v)) return MISSING;
  return decimals === 0 ? grouped.format(v) : fixed(v, decimals);
}

function Cell({
  p,
  col,
  cpuBarMaxPct,
  memUnits,
  rateUnits,
}: {
  p: LiveProcess;
  col: ProcessColumn;
  cpuBarMaxPct: number;
  memUnits: ByteUnits;
  rateUnits: RateUnits;
}) {
  switch (col) {
    case "name":
      return (
        <span className="inline-flex max-w-64 items-center gap-2 text-foreground">
          <InitialChip text={p.name} />
          <span className="truncate">{p.name}</span>
        </span>
      );
    case "pid":
      return <span className="data-mono text-muted-foreground">{p.pid}</span>;
    case "cpu": {
      const frac =
        p.cpu_pct == null
          ? 0
          : Math.min(1, Math.max(0, p.cpu_pct / cpuBarMaxPct));
      return (
        <span className="inline-flex items-center gap-2">
          <span className="inline-block h-1 w-12 overflow-hidden rounded-full bg-track">
            <span
              className="block h-full origin-left bg-cpu"
              style={{ transform: `scaleX(${frac})` }}
            />
          </span>
          <span className="data-mono inline-block w-10 text-right text-foreground">
            {num(p.cpu_pct, 1)}
          </span>
        </span>
      );
    }
    case "mem":
      return (
        <span className="data-mono">
          {formatBytes(p.mem_bytes, { units: memUnits })}
        </span>
      );
    case "compressed":
      return (
        <span className="data-mono">
          {formatBytes(p.compressed_bytes, { units: memUnits })}
        </span>
      );
    case "threads":
      return <span className="data-mono">{num(p.threads)}</span>;
    case "wakeups":
      return <span className="data-mono">{num(p.idle_wakeups_per_s)}</span>;
    case "energy":
      return <span className="data-mono">{num(p.energy, 1)}</span>;
    case "diskRead":
      return (
        <span className="data-mono">
          {formatRate(p.disk_read_bps, { units: rateUnits })}
        </span>
      );
    case "diskWrite":
      return (
        <span className="data-mono">
          {formatRate(p.disk_write_bps, { units: rateUnits })}
        </span>
      );
    case "diskTotal":
      return (
        <span className="data-mono text-foreground">
          {formatRate(diskTotal(p), { units: rateUnits })}
        </span>
      );
    case "netRx":
      return (
        <span className="data-mono">
          {formatRate(p.net_rx_bps, { units: rateUnits })}
        </span>
      );
    case "netTx":
      return (
        <span className="data-mono">
          {formatRate(p.net_tx_bps, { units: rateUnits })}
        </span>
      );
    case "netTotal":
      return (
        <span className="data-mono text-foreground">
          {formatRate(netTotal(p), { units: rateUnits })}
        </span>
      );
    case "gpu": {
      // Percent of the whole GPU (D-085), so the bar is out of 100.
      const frac =
        p.gpu_pct == null ? 0 : Math.min(1, Math.max(0, p.gpu_pct / 100));
      return (
        <span className="inline-flex items-center gap-2">
          <span className="inline-block h-1 w-12 overflow-hidden rounded-full bg-track">
            <span
              className="block h-full origin-left bg-gpu"
              style={{ transform: `scaleX(${frac})` }}
            />
          </span>
          <span className="data-mono inline-block w-10 text-right text-foreground">
            {num(p.gpu_pct, 1)}
          </span>
        </span>
      );
    }
    case "user":
      return <span className="data-mono text-muted-foreground">{p.user}</span>;
  }
}

/**
 * Sortable, virtualized process table (the CPU page's "Top processes"; full
 * height on the Processes page). Rows are keyed by (pid, start time). The parent owns
 * the sort; clicking the sorted column flips direction, another column sorts
 * descending (ascending for text).
 */
export function ProcessTable({
  rows,
  columns,
  sort,
  onSort,
  cpuBarMaxPct = 100,
  height,
  memUnits = "GB",
  rateUnits = "MBps",
  onSelect,
  rowAction,
  rowContextMenu,
  freezeOrderOnHover = false,
  growOnly = false,
  empty,
  ariaLabel,
}: ProcessTableProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [frozenKeys, setFrozenKeys] = useState<string[] | null>(null);
  const sortedLive = useMemo(() => sortProcesses(rows, sort), [rows, sort]);
  const sorted = useMemo(
    () => (frozenKeys ? frozenOrder(sortedLive, frozenKeys) : sortedLive),
    [sortedLive, frozenKeys]
  );
  const colSpan = columns.length + (rowAction ? 1 : 0);
  const showEmpty = rows.length === 0 && empty !== undefined;
  const bodyPx = Math.min((showEmpty ? 1 : rows.length) * ROW_H, height);
  const [tallestPx, setTallestPx] = useState(bodyPx);
  if (growOnly && bodyPx > tallestPx) setTallestPx(bodyPx);

  const virtualizer = useVirtualizer({
    count: sorted.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    getItemKey: (i) => processKey(sorted[i] as LiveProcess),
    overscan: 6,
    initialRect: { width: 0, height },
    // A window that has not been laid out yet (a hidden popover, a test DOM)
    // measures 0 px tall; render the viewport's worth of rows instead of none.
    observeElementRect: (instance, cb) =>
      observeElementRect(instance, (rect) =>
        cb(rect.height > 0 ? rect : { width: rect.width, height })
      ),
  });
  const items = virtualizer.getVirtualItems();
  const padTop = items[0]?.start ?? 0;
  const padBottom =
    virtualizer.getTotalSize() - (items[items.length - 1]?.end ?? 0);

  const clickHeader = (col: ProcessColumn) => {
    const next: ProcessSort =
      sort.by === col
        ? { by: col, dir: sort.dir === "asc" ? "desc" : "asc" }
        : { by: col, dir: TEXT_COLUMNS.has(col) ? "asc" : "desc" };
    // A sort the user asked for applies at once, then holds while hovering.
    if (frozenKeys) setFrozenKeys(sortProcesses(rows, next).map(processKey));
    onSort(next);
  };

  return (
    <div
      ref={scrollRef}
      data-testid="process-table-scroll"
      className="overflow-auto"
      style={
        growOnly
          ? { height: Math.max(tallestPx, bodyPx) + ROW_H + 4 }
          : { maxHeight: height + ROW_H + 4 }
      }
      onPointerEnter={
        freezeOrderOnHover
          ? () => setFrozenKeys(sortedLive.map(processKey))
          : undefined
      }
      onPointerLeave={
        freezeOrderOnHover ? () => setFrozenKeys(null) : undefined
      }
    >
      <table
        aria-label={ariaLabel}
        className="w-full border-collapse text-[12px]"
      >
        <thead className="sticky top-0 z-10 bg-card">
          <tr>
            {columns.map((col) => {
              const active = sort.by === col;
              const right = !TEXT_COLUMNS.has(col);
              return (
                <th
                  key={col}
                  scope="col"
                  aria-sort={
                    active
                      ? sort.dir === "asc"
                        ? "ascending"
                        : "descending"
                      : undefined
                  }
                  className={cn(
                    "whitespace-nowrap border-border border-b p-0 font-normal",
                    right ? "text-right" : "text-left"
                  )}
                >
                  <button
                    type="button"
                    onClick={() => clickHeader(col)}
                    className={cn(
                      "data-mono w-full px-3 py-2 text-[10px] uppercase tracking-[.08em] outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      right ? "text-right" : "text-left",
                      active
                        ? "text-foreground"
                        : "text-muted-foreground hover:text-foreground"
                    )}
                  >
                    {LABELS[col]}
                    {active && (sort.dir === "asc" ? " ↑" : " ↓")}
                  </button>
                </th>
              );
            })}
            {rowAction && (
              <th
                scope="col"
                className="w-px border-border border-b p-0 font-normal"
              >
                <span className="sr-only">Actions</span>
              </th>
            )}
          </tr>
        </thead>
        <tbody>
          {padTop > 0 && (
            <tr aria-hidden style={{ height: padTop }}>
              <td colSpan={colSpan} />
            </tr>
          )}
          {items.map((v) => {
            const p = sorted[v.index] as LiveProcess;
            const row = (
              <tr
                key={v.key}
                data-key={v.key}
                onClick={onSelect ? () => onSelect(p) : undefined}
                className={cn(
                  "group h-[29px] border-border-subtle border-b hover:bg-selected data-[state=open]:bg-selected",
                  onSelect && "cursor-default"
                )}
              >
                {columns.map((col) => (
                  <td
                    key={col}
                    className={cn(
                      "whitespace-nowrap px-3 py-0 font-normal text-fg-subtle",
                      !TEXT_COLUMNS.has(col) && "text-right"
                    )}
                  >
                    <Cell
                      p={p}
                      col={col}
                      cpuBarMaxPct={cpuBarMaxPct}
                      memUnits={memUnits}
                      rateUnits={rateUnits}
                    />
                  </td>
                ))}
                {rowAction && (
                  <td className="whitespace-nowrap py-0 pr-2 pl-1 text-right">
                    <span className="opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 group-data-[state=open]:opacity-100">
                      {rowAction(p)}
                    </span>
                  </td>
                )}
              </tr>
            );
            if (!rowContextMenu) return row;
            return (
              <ContextMenu key={v.key}>
                <ContextMenuTrigger asChild>{row}</ContextMenuTrigger>
                {rowContextMenu(p)}
              </ContextMenu>
            );
          })}
          {showEmpty && (
            <tr className="h-[29px]">
              <td colSpan={colSpan} className="px-3 py-0 text-muted-foreground">
                {empty}
              </td>
            </tr>
          )}
          {padBottom > 0 && (
            <tr aria-hidden style={{ height: padBottom }}>
              <td colSpan={colSpan} />
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
