import { formatPercent } from "@core/format";
import { Info } from "lucide-react";
import { type ReactNode, useId } from "react";
import { TableCell, TableHead, TableRow } from "~/components/ui/table";
import { cn } from "~/lib/utils";
import { Card } from "~/widgets/card";
import type { Accent } from "~/widgets/lib/accent";

/**
 * Parts of the per-app tables: the Network Apps card and the Power
 * page's energy table (D-093), which follows it.
 */

/** Cell padding and size shared by the per-app tables. */
export const APP_CELL = "px-3 py-1.75 text-[12px] whitespace-nowrap";

/**
 * The card shell: title with its scope, controls or the pinned
 * selection's chip on the right, then notes and the table edge to edge.
 */
export function AppsShell({
  title,
  aside,
  accent = "net",
  children,
}: {
  title: string;
  aside?: ReactNode;
  accent?: Accent;
  children?: ReactNode;
}) {
  const titleId = useId();
  return (
    <Card
      accent={accent}
      origin="tr"
      labelledBy={titleId}
      className="flex min-w-0 flex-col pb-1"
    >
      <div className="flex min-h-6 flex-wrap items-center gap-3 px-4 pt-3.5 pb-2">
        <h2 id={titleId} className="flex-1 font-[590] text-[14px]">
          {title}
        </h2>
        {aside}
      </div>
      {children}
    </Card>
  );
}

export function AppsNote({ children }: { children: ReactNode }) {
  return (
    <div className="px-4 pb-2">
      <p className="m-0 flex items-start gap-2 font-normal text-[12px] text-muted-foreground leading-normal">
        <Info aria-hidden className="mt-0.5 size-3.5 flex-none" />
        <span>{children}</span>
      </p>
    </div>
  );
}

/**
 * A range with nothing recorded in it: a hatched box with a chip saying so
 * (and, when known, when recording started).
 */
export function UnrecordedRange({ children }: { children: ReactNode }) {
  return (
    <div className="px-4 pt-1 pb-3.5">
      <div className="flex h-33 items-center justify-center rounded-lg border border-border-strong/40 border-dashed bg-[repeating-linear-gradient(135deg,var(--color-grid)_0_1px,transparent_1px_7px)]">
        <span className="rounded-md border border-border bg-card px-2 py-1 font-normal text-[11px] text-fg-subtle">
          {children}
        </span>
      </div>
    </div>
  );
}

/**
 * A remainder row after the apps ("Other apps", "System and other"): muted,
 * a rule above the first, a dashed square marking the system row. `indent`
 * leaves room for the expand chevron the app rows above have. `children`
 * are the figure cells.
 */
export function OtherAppsRow({
  name,
  system,
  first,
  indent = false,
  children,
}: {
  name: string;
  system: boolean;
  first: boolean;
  indent?: boolean;
  children: ReactNode;
}) {
  return (
    <TableRow
      className={cn(
        "text-muted-foreground",
        first && "border-border border-t",
        system && "border-b-0"
      )}
    >
      <TableCell className={APP_CELL}>
        <span className="inline-flex items-center gap-2">
          {indent && <span aria-hidden className="-ml-1 inline-block size-4" />}
          {system ? (
            <span
              aria-hidden
              className="inline-block size-4 flex-none rounded-[4px] border border-border-strong/50 border-dashed"
            />
          ) : (
            <span aria-hidden className="inline-block w-4 flex-none" />
          )}
          {name}
        </span>
      </TableCell>
      {children}
    </TableRow>
  );
}

export interface ColumnSort<K extends string> {
  by: K;
  dir: "asc" | "desc";
}

/** A sortable numeric column head: first click sorts descending. */
export function SortHead<K extends string>({
  by,
  label,
  sort,
  onSort,
}: {
  by: K;
  label: string;
  sort: ColumnSort<K>;
  onSort: (s: ColumnSort<K>) => void;
}) {
  const active = sort.by === by;
  return (
    <TableHead
      aria-sort={
        active ? (sort.dir === "asc" ? "ascending" : "descending") : undefined
      }
      className="p-0 text-right"
    >
      <button
        type="button"
        onClick={() =>
          onSort({
            by,
            dir: active && sort.dir === "desc" ? "asc" : "desc",
          })
        }
        className={cn(
          "w-full px-3 py-2 text-right uppercase tracking-[.08em] outline-none focus-visible:ring-2 focus-visible:ring-ring",
          active
            ? "text-foreground"
            : "text-muted-foreground hover:text-foreground"
        )}
      >
        {label}
        {active && (sort.dir === "desc" ? " ↓" : " ↑")}
      </button>
    </TableHead>
  );
}

/** A share of the whole, percent: a 40 px bar in the card accent and the figure. */
export function ShareCell({
  share,
  muted,
}: {
  share: number | null;
  muted: boolean;
}) {
  return (
    <TableCell className={cn(APP_CELL, "text-right")}>
      <span className="inline-flex items-center gap-2">
        <span className="inline-block h-1 w-10 overflow-hidden rounded-full bg-track">
          <span
            className={cn(
              "block h-full",
              muted ? "bg-fg-faint" : "bg-[var(--a)]"
            )}
            style={{ width: `${Math.min(100, Math.max(0, share ?? 0))}%` }}
          />
        </span>
        <span className="data-mono inline-block w-11 text-right">
          {formatPercent(share, { decimals: 1 })}
        </span>
      </span>
    </TableCell>
  );
}
