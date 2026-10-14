import type { ContainerSort } from "../_lib/presentation";

export function ContainerTableHead({
  sort,
  onSort,
  historical,
  columns,
}: {
  sort: ContainerSort;
  onSort: (sort: ContainerSort) => void;
  historical: boolean;
  columns: readonly string[];
}) {
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
  return (
    <thead className="sticky top-0 z-10 bg-card text-[11px] text-muted-foreground">
      <tr className="h-8 border-border border-y">
        <th className="w-5">
          <span className="sr-only">Chart color</span>
        </th>
        <th
          className={`w-[32%] px-2 text-left font-normal [@container(max-width:480px)]:w-auto`}
          aria-sort={sort === "name" ? "ascending" : "none"}
        >
          {heading("Name / project", "name")}
        </th>
        <th
          className={`px-2 text-left font-normal [@container(max-width:480px)]:hidden`}
        >
          State
        </th>
        <th
          className={`px-2 text-right font-normal [@container(max-width:480px)]:w-[52px]`}
          aria-sort={sort === "cpu" ? "descending" : "none"}
        >
          {heading(historical ? "Peak CPU" : "CPU", "cpu")}
        </th>
        <th
          className={`px-2 text-right font-normal [@container(max-width:480px)]:w-[92px]`}
          aria-sort={sort === "memory" ? "descending" : "none"}
        >
          {heading(historical ? "Peak memory" : "Memory", "memory")}
        </th>
        <th
          className={`px-2 text-right font-normal [@container(max-width:719px)]:hidden`}
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
  );
}
