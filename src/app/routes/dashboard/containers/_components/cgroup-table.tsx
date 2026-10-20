import { ratio } from "@core/chart-math";
import {
  type ByteUnits,
  fixed,
  formatBytes,
  formatPercent,
  formatRate,
  MISSING,
  type RateUnits,
} from "@core/format";
import { type ReactNode, useState } from "react";
import { type ColumnDef, SortHeader } from "~/components/sort-header";
import { cn } from "~/lib/utils";
import { InitialChip } from "~/widgets/initial-chip";
import { MeterTrack } from "~/widgets/meter-track";
import {
  type CgroupColumn,
  type CgroupRow,
  type CgroupSort,
  KIND_LABEL,
  sortCgroups,
} from "../_lib/cgroups";

interface CellCtx {
  bytes: ByteUnits;
  rate: RateUnits;
}

const figure = (text: string, className?: string) => (
  <span className={cn("figures", className)}>{text}</span>
);

/** A share with an inline bar, as the process table draws % CPU. */
function Bar({
  fraction,
  fill,
  children,
}: {
  fraction: number | null;
  fill: string;
  children: ReactNode;
}) {
  return (
    <span className="inline-flex items-center gap-2">
      <MeterTrack
        fraction={fraction}
        fill={fill}
        className="inline-block w-12"
      />
      {children}
    </span>
  );
}

const COLUMNS: Record<CgroupColumn, ColumnDef<CgroupRow, CellCtx>> = {
  name: {
    label: "Name",
    align: "left",
    sortValue: (r) => r.name,
    cell: (r) => (
      <span className="inline-flex max-w-72 items-center gap-2 text-foreground">
        <InitialChip text={r.name} />
        <span className="truncate" title={r.name}>
          {r.name}
        </span>
      </span>
    ),
  },
  kind: {
    label: "Kind",
    align: "left",
    sortValue: (r) => KIND_LABEL[r.kind],
    cell: (r) => figure(KIND_LABEL[r.kind], "text-muted-foreground"),
  },
  cpu: {
    // Percent of one core, so the bar fills at one core and the figure can pass 100.
    label: "% CPU",
    align: "right",
    sortValue: (r) => r.cpu,
    cell: (r) => (
      <Bar fraction={ratio(r.cpu, 100)} fill="var(--color-cpu)">
        {figure(
          r.cpu === null ? MISSING : fixed(r.cpu, 1),
          "inline-block w-10 text-right text-foreground"
        )}
      </Bar>
    ),
  },
  mem: {
    label: "Memory",
    align: "right",
    sortValue: (r) => r.mem,
    cell: (r, c) => {
      const used = figure(formatBytes(r.mem, { units: c.bytes }));
      if (r.memMax === null) return used;
      return (
        <Bar fraction={ratio(r.mem, r.memMax)} fill="var(--color-mem)">
          <span className="inline-block min-w-30 text-right">
            {used}
            {figure(
              ` / ${formatBytes(r.memMax, { units: c.bytes })}`,
              "text-muted-foreground"
            )}
          </span>
        </Bar>
      );
    },
  },
  ioRead: {
    label: "Disk read",
    align: "right",
    sortValue: (r) => r.ioRead,
    cell: (r, c) => figure(formatRate(r.ioRead, { units: c.rate })),
  },
  ioWrite: {
    label: "Disk write",
    align: "right",
    sortValue: (r) => r.ioWrite,
    cell: (r, c) => figure(formatRate(r.ioWrite, { units: c.rate })),
  },
  memPsi: {
    label: "Memory pressure",
    align: "right",
    sortValue: (r) => r.memPsi,
    cell: (r) => figure(formatPercent(r.memPsi, { decimals: 1 })),
  },
};

const CELL = "whitespace-nowrap px-3 py-0 font-normal text-fg-subtle";

export interface CgroupTableProps {
  rows: readonly CgroupRow[];
  /** The folded remainder, pinned last whatever the sort. */
  other: { label: string; row: CgroupRow } | null;
  columns: readonly CgroupColumn[];
  sort: CgroupSort;
  onSort: (sort: CgroupSort) => void;
  bytes: ByteUnits;
  rate: RateUnits;
  empty: ReactNode;
}

/**
 * cgroup groups on a Linux host, styled like the process table. Row order
 * holds while the pointer is over the table, as on Processes; a sort the
 * user picks applies at once.
 */
export function CgroupTable({
  rows,
  other,
  columns,
  sort,
  onSort,
  bytes,
  rate,
  empty,
}: CgroupTableProps) {
  const [frozen, setFrozen] = useState<string[] | null>(null);
  const sorted = sortCgroups(rows, sort);
  const shown = frozen ? holdOrder(sorted, frozen) : sorted;
  const ctx: CellCtx = { bytes, rate };
  const sortBy = (next: CgroupSort) => {
    if (frozen) setFrozen(sortCgroups(rows, next).map((r) => r.id));
    onSort(next);
  };
  return (
    <div
      className="min-h-0 overflow-auto"
      onPointerEnter={() => setFrozen(sorted.map((r) => r.id))}
      onPointerLeave={() => setFrozen(null)}
    >
      <table
        aria-label="Services and containers"
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
          </tr>
        </thead>
        <tbody>
          {shown.map((r) => (
            <tr
              key={r.id}
              className="h-[29px] border-border-subtle border-b hover:bg-selected"
            >
              {columns.map((col) => (
                <td
                  key={col}
                  className={cn(
                    CELL,
                    COLUMNS[col].align === "right" && "text-right"
                  )}
                >
                  {COLUMNS[col].cell(r, ctx)}
                </td>
              ))}
            </tr>
          ))}
          {shown.length === 0 && (
            <tr className="h-[29px]">
              <td
                colSpan={columns.length}
                className="px-3 py-0 text-muted-foreground"
              >
                {empty}
              </td>
            </tr>
          )}
          {other && (
            <OtherRow
              label={other.label}
              row={other.row}
              columns={columns}
              ctx={ctx}
            />
          )}
        </tbody>
      </table>
    </div>
  );
}

/** The remainder row, muted, as the per-app tables draw "Other apps". */
function OtherRow({
  label,
  row,
  columns,
  ctx,
}: {
  label: string;
  row: CgroupRow;
  columns: readonly CgroupColumn[];
  ctx: CellCtx;
}) {
  return (
    <tr className="h-[29px] border-border border-t text-muted-foreground">
      {columns.map((col) => (
        <td
          key={col}
          className={cn(
            CELL,
            "text-muted-foreground",
            COLUMNS[col].align === "right" && "text-right"
          )}
        >
          {col === "name" ? (
            <span className="inline-flex items-center gap-2">
              <span
                aria-hidden
                className="inline-block size-4 flex-none rounded-[4px] border border-border-strong/50 border-dashed"
              />
              <span className="truncate">{label}</span>
            </span>
          ) : col === "kind" || col === "memPsi" ? null : (
            COLUMNS[col].cell(row, ctx)
          )}
        </td>
      ))}
    </tr>
  );
}

/** `sorted` in the order of `ids`, new rows after them, gone ones dropped. */
function holdOrder(sorted: readonly CgroupRow[], ids: readonly string[]) {
  const byId = new Map(sorted.map((r) => [r.id, r]));
  const placed = new Set<string>();
  const out: CgroupRow[] = [];
  for (const id of ids) {
    const r = byId.get(id);
    if (r) {
      out.push(r);
      placed.add(id);
    }
  }
  for (const r of sorted) if (!placed.has(r.id)) out.push(r);
  return out;
}
