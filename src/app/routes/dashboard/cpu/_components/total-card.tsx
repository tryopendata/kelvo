import { fixed, formatPercent, MISSING } from "@core/format";
import { windowWords } from "@core/live-window";
import { useId } from "react";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useHeld } from "~/hooks/use-ring";
import { useWindowSeries } from "~/hooks/use-window-series";
import { Card } from "~/widgets/card";
import { windowTicks } from "~/widgets/lib/chart-labels";
import { StatStrip } from "~/widgets/stat-strip";
import { StreamArea } from "~/widgets/stream-area";

const LOADAVG = ["1", "5", "15"].map((w) => `cpu.loadavg{window=${w}}`);
const HELD = ["cpu.total", "cpu.user", "cpu.system", ...LOADAVG];
const CHART = ["cpu.total", "cpu.system"];
const Y_TICKS = [100, 75, 50, 25].map((v) => ({ value: v, label: String(v) }));

const load = (v: number | null | undefined) =>
  v == null ? MISSING : fixed(v, 2);

/**
 * Stat strip over the total and system area chart (top left), over
 * the chart window (D-091).
 */
export function TotalCard({ windowMs }: { windowMs: number }) {
  const titleId = useId();
  const v = useHeld(HELD);
  const total = v["cpu.total"] ?? null;
  const series = useWindowSeries(CHART, windowMs);
  const gaps = useGapBands("cpu");
  const [l1, l5, l15] = LOADAVG.map((k) => v[k] ?? null);

  return (
    <Card
      accent="cpu"
      variant="chart"
      labelledBy={titleId}
      className="col-span-2 flex flex-col gap-3.5 p-4"
    >
      <h2 id={titleId} className="sr-only">
        CPU total
      </h2>
      <StatStrip
        hero={{ label: "Total", value: formatPercent(total) }}
        items={[
          {
            label: "User",
            value: formatPercent(v["cpu.user"], { decimals: 1 }),
          },
          {
            label: "System",
            value: formatPercent(v["cpu.system"], { decimals: 1 }),
            swatch: 2,
          },
          {
            label: "Idle",
            value: formatPercent(total === null ? null : 100 - total, {
              decimals: 1,
            }),
            muted: true,
          },
          {
            label: "Load avg",
            value: load(l1),
            secondary: `${load(l5)} ${load(l15)}`,
          },
        ]}
      />
      <StreamArea
        gaps={gaps}
        series={[
          {
            key: "cpu.total",
            values: series.values["cpu.total"] ?? [],
            step: 1,
          },
          {
            key: "cpu.system",
            values: series.values["cpu.system"] ?? [],
            step: 2,
          },
        ]}
        tEndMs={series.tEndMs}
        intervalMs={series.intervalMs}
        yMax={100}
        accent="cpu"
        height={220}
        ariaLabel={`CPU total and system, last ${windowWords(windowMs)}, total ${formatPercent(total)}`}
        gridlines={[25, 50, 75, 100]}
        yTicks={Y_TICKS}
        xTicks={windowTicks(windowMs, 5)}
      />
    </Card>
  );
}
