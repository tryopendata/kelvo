import { formatRate } from "@core/format";
import type { LiveProcess } from "@core/generated/bindings";
import { labelValues } from "@core/series-key";
import { useMemo, useState } from "react";
import { Link } from "react-router";
import { LiveMirrorChart } from "~/components/live-mirror-chart";
import { PageHeader } from "~/components/page-header";
import { type ProcessSort, ProcessTable } from "~/components/process-table";
import { SectionCard } from "~/components/section-card";
import { VolumeTable } from "~/components/volume-table";
import { WindowControl } from "~/components/window-control";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useHostRecord } from "~/hooks/use-host-record";
import {
  useProcessInterest,
  useProcessRows,
} from "~/hooks/use-process-interest";
import { useHeld, useLayout } from "~/hooks/use-ring";
import { useDisk } from "~/stores/live-selectors";
import { Card } from "~/widgets/card";
import { StatStrip } from "~/widgets/stat-strip";
import { diskSubtitle, volumeRows } from "./_lib/disk";

const NO_ROWS: readonly LiveProcess[] = [];
const NO_MOUNTS: readonly string[] = [];
/** The CPU page's process table shows 8 rows; 29 px each. */
const PROCESS_ROWS_PX = 8 * 29;

/**
 * Disk (plan 4.12): the Overview Disk card and the popover's mirrored
 * bars at page size, laid out like the CPU page: throughput chart first, then
 * volumes, then processes by disk I/O. The cards wait for the chart window
 * (D-091), so they appear together.
 */
export default function DiskRoute() {
  useProcessInterest();
  // Null until settings load: the page waits rather than draw a span it would redo.
  const windowMs = useChartWindow()?.windowMs ?? null;
  const gaps = useGapBands("disk");
  const [sort, setSort] = useState<ProcessSort>({
    by: "diskTotal",
    dir: "desc",
  });
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
  const processes = useProcessRows() ?? NO_ROWS;
  const rate = (v: number | null) => formatRate(v);

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Disk"
        subtitle={diskSubtitle(devices, vols.length)}
        actions={<WindowControl />}
      />
      {windowMs !== null && (
        <>
          <Card
            accent="disk"
            variant="chart"
            labelledBy="disk-throughput"
            className="flex flex-col gap-4 p-4"
          >
            <h2 id="disk-throughput" className="sr-only">
              Throughput
            </h2>
            <StatStrip
              hero={{ label: "Read", value: rate(disk.read) }}
              items={[{ label: "Write", value: rate(disk.write) }]}
            />
            <LiveMirrorChart
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
          </Card>
          <SectionCard
            accent="disk"
            origin="tr"
            title="Volumes"
            variant="default"
          >
            <VolumeTable rows={volumeRows(vols, held, bootMounts)} units="GB" />
          </SectionCard>
          <SectionCard
            accent="disk"
            origin="bl"
            title="Processes by disk I/O"
            variant="default"
            aside={
              <Link
                to="/dashboard/processes"
                className="rounded-chip font-normal text-[11px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              >
                Show all
              </Link>
            }
          >
            <ProcessTable
              rows={processes}
              columns={[
                "name",
                "pid",
                "diskRead",
                "diskWrite",
                "diskTotal",
                "user",
              ]}
              sort={sort}
              onSort={setSort}
              height={PROCESS_ROWS_PX}
              ariaLabel="Processes by disk I/O"
            />
          </SectionCard>
        </>
      )}
    </div>
  );
}
