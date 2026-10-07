import { formatPercent } from "@core/format";
import { Info } from "lucide-react";
import { type ReactNode, useId } from "react";
import { TableCell, TableHead } from "~/components/ui/table";
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

/** The app's first letter in a 16 px tile, so names scan as a list. */
export function AppInitial({ name }: { name: string }) {
  return (
    <span
      aria-hidden
      className="data-mono inline-flex size-4 flex-none items-center justify-center rounded-[4px] bg-raised text-[9px] text-fg-subtle"
    >
      {name.slice(0, 1)}
    </span>
  );
}
