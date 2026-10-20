/**
 * A Linux host's Containers table from its `cgroup.*` series (D-115). Rust
 * decides which groups get a series, folds the rest into `_other` and counts
 * them in `cgroup.other_count`; this only lays the current layout out as
 * rows, sorts and filters them.
 */
import type { LayoutInfo } from "@core/live-state";
import { labelOf, sk } from "@core/series-key";

/** The `kind` label values `linux/cgroup.rs` writes. */
export type CgroupKind = "service" | "docker" | "podman" | "user" | "other";

export const KIND_LABEL: Record<CgroupKind, string> = {
  service: "Service",
  docker: "Docker",
  podman: "Podman",
  user: "User service",
  other: "Other",
};

/** The `name` of the folded remainder (`cgroup::OTHER`). */
export const OTHER_NAME = "_other";

const METRICS = {
  cpu: "cgroup.cpu",
  mem: "cgroup.mem",
  memMax: "cgroup.mem_max",
  ioRead: "cgroup.io_read",
  ioWrite: "cgroup.io_write",
  memPsi: "cgroup.mem_psi",
} as const;

type Field = keyof typeof METRICS;

/** One group in the layout: the display key of each series it has. */
export interface CgroupGroup {
  /** `kind/name`, the row key. */
  id: string;
  kind: CgroupKind;
  name: string;
  keys: Partial<Record<Field, string>>;
}

export interface CgroupLayout {
  groups: CgroupGroup[];
  /** The folded remainder, when Rust folded any groups. */
  other: CgroupGroup | null;
  /** Some group has disk I/O (the io controller is on). */
  io: boolean;
  /** Some group has memory PSI (`memory.pressure`). */
  psi: boolean;
  /** Every key a row reads, for one shallow `useHeld`. */
  keys: string[];
}

export const OTHER_COUNT_KEY = "cgroup.other_count";

const KINDS = new Set<string>(Object.keys(KIND_LABEL));

/** The groups of `layout`, in layout order, each with the series it has. */
export function cgroupLayout(layout: LayoutInfo | null): CgroupLayout {
  const byId = new Map<string, CgroupGroup>();
  const fields = Object.entries(METRICS) as [Field, string][];
  for (const series of layout?.series ?? []) {
    const field = fields.find(([, m]) => m === series.metric)?.[0];
    if (!field) continue;
    const kind = labelOf(series, "kind");
    const name = labelOf(series, "name");
    if (kind === undefined || name === undefined || !KINDS.has(kind)) continue;
    const id = `${kind}/${name}`;
    let group = byId.get(id);
    if (!group) {
      group = { id, kind: kind as CgroupKind, name, keys: {} };
      byId.set(id, group);
    }
    group.keys[field] = sk(series.metric, { kind, name });
  }
  const all = [...byId.values()];
  const other =
    all.find((g) => g.kind === "other" && g.name === OTHER_NAME) ?? null;
  const groups = all.filter((g) => g !== other);
  const keys = all.flatMap((g) => Object.values(g.keys));
  if (layout?.index.has(OTHER_COUNT_KEY)) keys.push(OTHER_COUNT_KEY);
  return {
    groups,
    other,
    io: all.some((g) => g.keys.ioRead || g.keys.ioWrite),
    psi: all.some((g) => g.keys.memPsi),
    keys,
  };
}

/** A group's current values; `null` is a gap (an exited container), never 0. */
export interface CgroupRow {
  id: string;
  kind: CgroupKind;
  name: string;
  cpu: number | null;
  mem: number | null;
  memMax: number | null;
  ioRead: number | null;
  ioWrite: number | null;
  memPsi: number | null;
}

export function cgroupRow(
  group: CgroupGroup,
  held: Record<string, number | null>
): CgroupRow {
  const value = (field: Field) => {
    const key = group.keys[field];
    return key === undefined ? null : (held[key] ?? null);
  };
  return {
    id: group.id,
    kind: group.kind,
    name: group.name,
    cpu: value("cpu"),
    mem: value("mem"),
    memMax: value("memMax"),
    ioRead: value("ioRead"),
    ioWrite: value("ioWrite"),
    memPsi: value("memPsi"),
  };
}

/** The table's kind filter: units and user services, or Docker and Podman. */
export type CgroupFilter = "all" | "services" | "containers";

export const FILTER_OPTIONS: { value: CgroupFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "services", label: "Services" },
  { value: "containers", label: "Containers" },
];

export function inFilter(kind: CgroupKind, filter: CgroupFilter): boolean {
  if (filter === "services") return kind === "service" || kind === "user";
  if (filter === "containers") return kind === "docker" || kind === "podman";
  return true;
}

export type CgroupColumn =
  | "name"
  | "kind"
  | "cpu"
  | "mem"
  | "ioRead"
  | "ioWrite"
  | "memPsi";

export interface CgroupSort {
  by: CgroupColumn;
  dir: "asc" | "desc";
}

export const DEFAULT_SORT: CgroupSort = { by: "cpu", dir: "desc" };

function sortValue(row: CgroupRow, by: CgroupColumn): number | string | null {
  if (by === "name") return row.name.toLowerCase();
  if (by === "kind") return KIND_LABEL[row.kind];
  return row[by];
}

/** Sort a copy; a gap sorts last in either direction, ties by name. */
export function sortCgroups(
  rows: readonly CgroupRow[],
  { by, dir }: CgroupSort
): CgroupRow[] {
  const sign = dir === "asc" ? 1 : -1;
  const byName = (a: CgroupRow, b: CgroupRow) => a.name.localeCompare(b.name);
  return [...rows].sort((a, b) => {
    const va = sortValue(a, by);
    const vb = sortValue(b, by);
    if (va === null && vb === null) return byName(a, b);
    if (va === null) return 1;
    if (vb === null) return -1;
    if (va < vb) return -sign;
    if (va > vb) return sign;
    return byName(a, b);
  });
}

/** "25 others"; "Others" while the count is a gap. */
export function othersLabel(count: number | null): string {
  if (count === null) return "Others";
  const n = Math.round(count);
  return `${n} ${n === 1 ? "other" : "others"}`;
}
