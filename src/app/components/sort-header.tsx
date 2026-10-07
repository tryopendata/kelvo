import type { ReactNode } from "react";
import { TableHead } from "~/components/ui/table";
import { cn } from "~/lib/utils";

export type SortDir = "asc" | "desc";

export interface ColumnSort<K extends string> {
  by: K;
  dir: SortDir;
}

/**
 * A table column as data: its head, alignment, how a row renders in it and
 * the value it sorts by (`null` sorts last). `C` is what every cell reads
 * besides its row (units, a bar's scale).
 */
export interface ColumnDef<T, C = void> {
  label: string;
  align: "left" | "right";
  cell: (row: T, ctx: C) => ReactNode;
  sortValue: (row: T) => number | string | null;
}

type SortHeaderProps<K extends string> = {
  label: string;
  align?: "left" | "right";
  className?: string;
} & (
  | {
      by: K;
      sort: ColumnSort<K>;
      onSort: (next: ColumnSort<K>) => void;
      /** Direction of the first click on this column. */
      firstDir?: SortDir;
      ranked?: never;
    }
  | {
      /** A fixed order the table is ranked by (Rust sorts it): not a button. */
      ranked: true;
      by?: never;
      sort?: never;
      onSort?: never;
      firstDir?: never;
    }
);

/**
 * A column head (field label) that sorts its table: the active column shows
 * its direction and `aria-sort`; a click on it flips the direction, a click
 * on another column sorts by that one in `firstDir`.
 */
export function SortHeader<K extends string>(props: SortHeaderProps<K>) {
  const { label, align = "right", className } = props;
  const right = align === "right";
  const dir: SortDir | null = props.ranked
    ? "desc"
    : props.sort.by === props.by
      ? props.sort.dir
      : null;
  const arrow = dir && <span aria-hidden>{dir === "desc" ? " ↓" : " ↑"}</span>;
  return (
    <TableHead
      scope="col"
      aria-sort={
        dir === null ? undefined : dir === "asc" ? "ascending" : "descending"
      }
      className={cn(
        right && "text-right",
        props.ranked ? "text-foreground" : "p-0",
        className
      )}
    >
      {props.ranked ? (
        <>
          {label}
          {arrow}
        </>
      ) : (
        <button
          type="button"
          onClick={() => {
            const { by, sort, onSort, firstDir = "desc" } = props;
            onSort({
              by,
              dir:
                sort.by === by
                  ? sort.dir === "asc"
                    ? "desc"
                    : "asc"
                  : firstDir,
            });
          }}
          // `uppercase` again: preflight resets text-transform on buttons.
          className={cn(
            "w-full px-3 py-2 uppercase outline-none focus-visible:ring-2 focus-visible:ring-ring",
            right ? "text-right" : "text-left",
            dir
              ? "text-foreground"
              : "text-muted-foreground hover:text-foreground"
          )}
        >
          {label}
          {arrow}
        </button>
      )}
    </TableHead>
  );
}
