import { ratio } from "@core/chart-math";
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
import { type ColumnDef, SortHeader } from "~/components/sort-header";
import { ContextMenu, ContextMenuTrigger } from "~/components/ui/context-menu";
import { cn } from "~/lib/utils";
import { InitialChip } from "~/widgets/initial-chip";
import { MeterTrack } from "~/widgets/meter-track";

export type ProcessColumn =
  | "name"
  | "pid"
  | "port"
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
    const value = COLUMNS[by].sortValue;
    const va = value(a);
    const vb = value(b);
    if (va == null && vb == null) return a.pid - b.pid;
    if (va == null) return 1;
    if (vb == null) return -1;
    if (va < vb) return -sign;
    if (va > vb) return sign;
    return a.pid - b.pid;
  });
}

const grouped = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });

/** Ports listed in a cell before the rest collapse to "+N". */
const PORTS_SHOWN = 3;

/**
 * "3000, 5173", or "5432, 6379, 8080 +4" past three. Blank for a process
 * listening on none, which is most rows; a dash only when ports were not read.
 */
export function portsText(ports: readonly number[] | null): string {
  if (ports == null) return MISSING;
  const head = ports.slice(0, PORTS_SHOWN).join(", ");
  const rest = ports.length - PORTS_SHOWN;
  return rest > 0 ? `${head} +${rest}` : head;
}

function num(v: number | null, decimals = 0): string {
  if (v == null || !Number.isFinite(v)) return MISSING;
  return decimals === 0 ? grouped.format(v) : fixed(v, decimals);
}

/** What a process cell reads besides its row. */
interface CellCtx {
  cpuBarMaxPct: number;
  memUnits: ByteUnits;
  rateUnits: RateUnits;
}

const figure = (text: string, className?: string) => (
  <span className={cn("figures", className)}>{text}</span>
);

/** A percent with an inline bar in the module color, the bar full at `max`. */
function PercentBar({
  pct,
  max,
  fill,
}: {
  pct: number | null;
  max: number;
  fill: string;
}) {
  return (
    <span className="inline-flex items-center gap-2">
      <MeterTrack
        fraction={ratio(pct, max)}
        fill={fill}
        className="inline-block w-12"
      />
      <span className="figures inline-block w-10 text-right text-foreground">
        {num(pct, 1)}
      </span>
    </span>
  );
}

const COLUMNS: Record<ProcessColumn, ColumnDef<LiveProcess, CellCtx>> = {
  name: {
    label: "Process",
    align: "left",
    sortValue: (p) => p.name.toLowerCase(),
    cell: (p) => (
      <span className="inline-flex max-w-64 items-center gap-2 text-foreground">
        <InitialChip text={p.name} />
        <span className="truncate">{p.name}</span>
      </span>
    ),
  },
  pid: {
    label: "PID",
    align: "right",
    sortValue: (p) => p.pid,
    cell: (p) => figure(String(p.pid), "text-muted-foreground"),
  },
  port: {
    label: "Port",
    align: "right",
    // The lowest port; a process listening on none sorts with the unread.
    sortValue: (p) => p.ports?.[0] ?? null,
    cell: (p) => (
      <span
        className="figures text-muted-foreground"
        title={
          p.ports && p.ports.length > PORTS_SHOWN
            ? p.ports.join(", ")
            : undefined
        }
      >
        {portsText(p.ports)}
      </span>
    ),
  },
  cpu: {
    label: "% CPU",
    align: "right",
    sortValue: (p) => p.cpu_pct,
    cell: (p, c) => (
      <PercentBar
        pct={p.cpu_pct}
        max={c.cpuBarMaxPct}
        fill="var(--color-cpu)"
      />
    ),
  },
  mem: {
    label: "Memory",
    align: "right",
    sortValue: (p) => p.mem_bytes,
    cell: (p, c) => figure(formatBytes(p.mem_bytes, { units: c.memUnits })),
  },
  compressed: {
    label: "Compressed",
    align: "right",
    sortValue: (p) => p.compressed_bytes,
    cell: (p, c) =>
      figure(formatBytes(p.compressed_bytes, { units: c.memUnits })),
  },
  threads: {
    label: "Threads",
    align: "right",
    sortValue: (p) => p.threads,
    cell: (p) => figure(num(p.threads)),
  },
  wakeups: {
    label: "Idle wake-ups",
    align: "right",
    sortValue: (p) => p.idle_wakeups_per_s,
    cell: (p) => figure(num(p.idle_wakeups_per_s)),
  },
  energy: {
    label: "Energy",
    align: "right",
    sortValue: (p) => p.energy,
    cell: (p) => figure(num(p.energy, 1)),
  },
  diskRead: {
    label: "Disk read",
    align: "right",
    sortValue: (p) => p.disk_read_bps,
    cell: (p, c) => figure(formatRate(p.disk_read_bps, { units: c.rateUnits })),
  },
  diskWrite: {
    label: "Disk write",
    align: "right",
    sortValue: (p) => p.disk_write_bps,
    cell: (p, c) =>
      figure(formatRate(p.disk_write_bps, { units: c.rateUnits })),
  },
  diskTotal: {
    label: "Disk total",
    align: "right",
    sortValue: diskTotal,
    cell: (p, c) =>
      figure(
        formatRate(diskTotal(p), { units: c.rateUnits }),
        "text-foreground"
      ),
  },
  netRx: {
    label: "Down",
    align: "right",
    sortValue: (p) => p.net_rx_bps,
    cell: (p, c) => figure(formatRate(p.net_rx_bps, { units: c.rateUnits })),
  },
  netTx: {
    label: "Up",
    align: "right",
    sortValue: (p) => p.net_tx_bps,
    cell: (p, c) => figure(formatRate(p.net_tx_bps, { units: c.rateUnits })),
  },
  netTotal: {
    label: "Net total",
    align: "right",
    sortValue: netTotal,
    cell: (p, c) =>
      figure(
        formatRate(netTotal(p), { units: c.rateUnits }),
        "text-foreground"
      ),
  },
  gpu: {
    label: "% GPU",
    align: "right",
    sortValue: (p) => p.gpu_pct,
    // Percent of the whole GPU (D-085), so the bar is out of 100.
    cell: (p) => (
      <PercentBar pct={p.gpu_pct} max={100} fill="var(--color-gpu)" />
    ),
  },
  user: {
    label: "User",
    align: "left",
    sortValue: (p) => p.user,
    cell: (p) => figure(p.user, "text-muted-foreground"),
  },
};

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

  const ctx: CellCtx = { cpuBarMaxPct, memUnits, rateUnits };
  const sortBy = (next: ProcessSort) => {
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
              const { label, align } = COLUMNS[col];
              return (
                <SortHeader
                  key={col}
                  by={col}
                  label={label}
                  align={align}
                  sort={sort}
                  onSort={sortBy}
                  firstDir={align === "left" ? "asc" : "desc"}
                  className="h-auto whitespace-nowrap border-border border-b"
                />
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
                      COLUMNS[col].align === "right" && "text-right"
                    )}
                  >
                    {COLUMNS[col].cell(p, ctx)}
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
