import { fixed, formatPercent, MISSING } from "@core/format";
import { windowWords } from "@core/live-window";
import { useId } from "react";
import { useAreaBrush } from "~/components/area-brush";
import { brushScopeProps } from "~/components/brush-overlay";
import {
  type RangeTotal,
  RangeTotalsStrip,
} from "~/components/range-totals-strip";
import { SelectionSummary } from "~/components/selection-summary";
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
const HEIGHT = 220;
/** Over the window or the selection (D-099): `cpu.total` averaged and its peak. */
const TOTALS: readonly RangeTotal[] = [
  {
    metric: "cpu.total",
    label: "Avg",
    format: (s) => formatPercent(s.avg, { decimals: 1 }),
  },
  { metric: "cpu.total", label: "Peak", format: (s) => formatPercent(s.max) },
];

const load = (v: number | null | undefined) =>
  v == null ? MISSING : fixed(v, 2);

/**
 * Stat strip over the total and system area chart (top left), over
 * the chart window (D-091). The chart is brushable (D-099): its average and
 * peak beside the live strip, and the apps table below, follow the selection.
 */
export function TotalCard({ windowMs }: { windowMs: number }) {
  const titleId = useId();
  const v = useHeld(HELD);
  const total = v["cpu.total"] ?? null;
  const series = useWindowSeries(CHART, windowMs, { brush: true });
  const brush = useAreaBrush(series, HEIGHT);
  const gaps = useGapBands("cpu");
  const [l1, l5, l15] = LOADAVG.map((k) => v[k] ?? null);

  return (
    <div {...brushScopeProps} className="contents">
      <Card
        accent="cpu"
        variant="chart"
        labelledBy={titleId}
        className="col-span-2 flex flex-col gap-3.5 p-4"
      >
        <h2 id={titleId} className="sr-only">
          CPU total
        </h2>
        <div className="flex flex-wrap items-end justify-between gap-x-7 gap-y-3">
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
          <RangeTotalsStrip
            windowMs={windowMs}
            totals={TOTALS}
            testId="cpu-range-totals"
          />
        </div>
        <div className="flex flex-col gap-1.5">
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
            height={HEIGHT}
            ariaLabel={`CPU total and system, last ${windowWords(windowMs)}, total ${formatPercent(total)}. Drag to select a range.`}
            gridlines={[25, 50, 75, 100]}
            yTicks={Y_TICKS}
            xTicks={windowTicks(windowMs, 5)}
            highlight={brush.highlight}
            overlay={brush.overlay}
          />
          <SelectionSummary windowMs={windowMs} inset={32} />
        </div>
      </Card>
    </div>
  );
}
