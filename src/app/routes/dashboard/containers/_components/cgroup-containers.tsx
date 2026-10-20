import { METRIC_PERIODS_MS } from "@core/generated/bindings";
import { useMemo, useState } from "react";
import { PageHeader } from "~/components/page-header";
import { SectionCard } from "~/components/section-card";
import { SegmentedControl } from "~/components/segmented-control";
import { useHeld, useLayout } from "~/hooks/use-ring";
import { useUnits } from "~/hooks/use-units";
import {
  type CgroupColumn,
  type CgroupFilter,
  type CgroupSort,
  cgroupLayout,
  cgroupRow,
  DEFAULT_SORT,
  FILTER_OPTIONS,
  inFilter,
  OTHER_COUNT_KEY,
  othersLabel,
} from "../_lib/cgroups";
import { CgroupTable } from "./cgroup-table";

const PERIOD_S = METRIC_PERIODS_MS["cgroup.cpu"] / 1000;

/**
 * Containers on a Linux host (D-116): the agent's cgroup v2 services and
 * containers from their `cgroup.*` series, the top groups with the rest
 * folded into one "others" row pinned last. A container that exited reads
 * as a gap until Rust drops it from the layout.
 */
export function CgroupContainers() {
  const layout = useLayout();
  const groups = useMemo(() => cgroupLayout(layout), [layout]);
  const held = useHeld(groups.keys);
  const units = useUnits();
  const [filter, setFilter] = useState<CgroupFilter>("all");
  const [sort, setSort] = useState<CgroupSort>(DEFAULT_SORT);

  const columns: CgroupColumn[] = [
    "name",
    "kind",
    "cpu",
    "mem",
    ...(groups.io ? (["ioRead", "ioWrite"] as const) : []),
    ...(groups.psi ? (["memPsi"] as const) : []),
  ];
  const all = groups.groups.map((g) => cgroupRow(g, held));
  const rows = all.filter((r) => inFilter(r.kind, filter));
  const otherCount = held[OTHER_COUNT_KEY] ?? null;
  // The remainder mixes kinds, so it belongs to the unfiltered table only.
  const other =
    groups.other && filter === "all"
      ? { label: othersLabel(otherCount), row: cgroupRow(groups.other, held) }
      : null;
  const count =
    rows.length === all.length
      ? `${all.length} groups`
      : `${rows.length} of ${all.length} groups`;
  const folded =
    groups.other && otherCount !== null
      ? `, ${Math.round(otherCount)} more in others`
      : "";

  return (
    <div className="flex h-full min-h-[420px] flex-col gap-4">
      <PageHeader
        title="Containers"
        subtitle={`Services and containers from cgroup v2 · every ${PERIOD_S} s`}
        actions={
          <SegmentedControl
            options={FILTER_OPTIONS}
            value={filter}
            onChange={setFilter}
            ariaLabel="Show"
          />
        }
      />
      <SectionCard
        accent="cpu"
        title="Services and containers"
        hiddenTitle
        flush
        className="min-h-0 flex-1"
      >
        <div className="min-h-0 flex-1 px-1 pt-1">
          <CgroupTable
            rows={rows}
            other={other}
            columns={columns}
            sort={sort}
            onSort={setSort}
            bytes={units.bytes}
            rate={units.rate}
            empty={
              layout === null
                ? "Waiting for the first sample…"
                : filter === "containers"
                  ? "No containers running."
                  : "No services reported."
            }
          />
        </div>
        <footer className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 border-border-subtle border-t px-4 py-2.5 font-normal text-[11px] text-muted-foreground">
          <span className="figures">
            {count}
            {folded}
          </span>
          <span>CPU is percent of one core.</span>
        </footer>
      </SectionCard>
    </div>
  );
}
