import { formatBytes, formatRate } from "@core/format";
import type { MetricStat } from "@core/generated/bindings";
import { labelValues } from "@core/series-key";
import { useMemo } from "react";
import { useNavigate } from "react-router";
import { brushScopeProps } from "~/components/brush-overlay";
import { LiveMirrorChart } from "~/components/live-mirror-chart";
import { PageHeader } from "~/components/page-header";
import {
  type RangeTotal,
  RangeTotalsStrip,
} from "~/components/range-totals-strip";
import { SectionCard } from "~/components/section-card";
import { SelectionSummary } from "~/components/selection-summary";
import {
  UsageAppsCard,
  type UsageCells,
  type UsageTableConfig,
} from "~/components/usage-apps-card";
import { VolumeTable } from "~/components/volume-table";
import { WindowControl } from "~/components/window-control";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useHostRecord } from "~/hooks/use-host-record";
import { useHeld, useLayout } from "~/hooks/use-ring";
import { BrushProvider } from "~/stores/brush-store";
import { useDisk } from "~/stores/live-selectors";
import { Card } from "~/widgets/card";
import { StatStrip } from "~/widgets/stat-strip";
import { diskSubtitle, volumeRows } from "./_lib/disk";

const NO_MOUNTS: readonly string[] = [];
/** Bytes moved over the window or the selection (D-099): the device totals' integrals. */
const TOTALS: readonly RangeTotal[] = [
  {
    metric: "disk.read_total",
    label: "Read",
    format: (s: MetricStat) => formatBytes(s.integral),
  },
  {
    metric: "disk.write_total",
    label: "Written",
    format: (s: MetricStat) => formatBytes(s.integral),
  },
];
const bytes = (n: number | null) => formatBytes(n);
const total = (u: UsageCells) =>
  u.read_bytes === null && u.write_bytes === null
    ? bytes(null)
    : bytes((u.read_bytes ?? 0) + (u.write_bytes ?? 0));

/**
 * Disk (plan 4.12): the Overview Disk card and the popover's mirrored
 * bars at page size, laid out like the CPU page: throughput chart first, then
 * volumes, then apps by bytes read and written. The cards wait for the chart
 * window (D-091), so they appear together. A range brushed on the chart scopes
 * the byte totals and the apps table (D-099).
 */
export default function DiskRoute() {
  // Null until settings load: the page waits rather than draw a span it would redo.
  const windowMs = useChartWindow()?.windowMs ?? null;
  const gaps = useGapBands("disk");
  const navigate = useNavigate();
  const disk = useDisk();
  const layout = useLayout();
  const series = layout?.series;
  const devices = useMemo(
    () => labelValues(series ?? [], "disk.read", "dev"),
    [series]
  );
  const vols = useMemo(
    () => labelValues(series ?? [], "disk.total", "vol"),
    [series]
  );
  const volKeys = useMemo(
    () =>
      ["disk.used", "disk.free", "disk.total"].flatMap(
        (m) => layout?.byMetric.get(m) ?? []
      ),
    [layout]
  );
  const held = useHeld(volKeys);
  const bootMounts = useHostRecord()?.info.boot_mounts ?? NO_MOUNTS;
  const table = useMemo(
    (): UsageTableConfig => ({
      by: "disk",
      noun: "Disk",
      accent: "disk",
      columns: [
        { label: "Read", format: (u) => bytes(u.read_bytes) },
        { label: "Written", format: (u) => bytes(u.write_bytes) },
        { label: "Total", format: total, ranked: true },
      ],
      footnote: (
        <>
          Bytes each process asked the disk for, as macOS counts them. System
          and other is the rest of what the disks moved: other users' and macOS
          processes, and file system work no process is charged for.{" "}
          <button
            type="button"
            onClick={() => navigate("/dashboard/processes")}
            className="rounded-sm text-fg-subtle underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring"
          >
            Every process
          </button>
        </>
      ),
    }),
    [navigate]
  );
  const rate = (v: number | null) => formatRate(v);

  return (
    <BrushProvider>
      <div className="flex flex-col gap-4">
        <PageHeader
          title="Disk"
          subtitle={diskSubtitle(devices, vols.length)}
          actions={<WindowControl />}
        />
        {windowMs !== null && (
          <>
            <div {...brushScopeProps} className="contents">
              <Card
                accent="disk"
                variant="chart"
                labelledBy="disk-throughput"
                className="flex flex-col gap-4 p-4"
              >
                <h2 id="disk-throughput" className="sr-only">
                  Throughput
                </h2>
                <div className="flex flex-wrap items-end justify-between gap-x-7 gap-y-3">
                  <StatStrip
                    hero={{ label: "Read", value: rate(disk.read) }}
                    items={[{ label: "Write", value: rate(disk.write) }]}
                  />
                  <RangeTotalsStrip
                    windowMs={windowMs}
                    totals={TOTALS}
                    testId="disk-range-totals"
                  />
                </div>
                <div className="flex flex-col gap-1.5">
                  <LiveMirrorChart
                    brush
                    gaps={gaps}
                    upKey="disk.read_total"
                    downKey="disk.write_total"
                    upLabel="Read"
                    downLabel="Write"
                    windowMs={windowMs}
                    accent="disk"
                    format={(v) => formatRate(v)}
                    minCeiling={1_000_000}
                    upHeight={96}
                    downHeight={96}
                  />
                  <SelectionSummary windowMs={windowMs} />
                </div>
              </Card>
            </div>
            <SectionCard
              accent="disk"
              origin="tr"
              title="Volumes"
              variant="default"
            >
              <VolumeTable
                rows={volumeRows(vols, held, bootMounts)}
                units="GB"
              />
            </SectionCard>
            <UsageAppsCard windowMs={windowMs} config={table} />
          </>
        )}
      </div>
    </BrushProvider>
  );
}
