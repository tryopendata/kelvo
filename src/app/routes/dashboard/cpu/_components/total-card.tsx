import { fixed, formatPercent, MISSING } from "@core/format";
import { windowWords } from "@core/live-window";
import { useAreaBrush } from "~/components/area-brush";
import { brushScopeProps } from "~/components/brush-overlay";
import {
  type RangeTotal,
  RangeTotalsStrip,
} from "~/components/range-totals-strip";
import { SectionCard } from "~/components/section-card";
import { SelectionSummary } from "~/components/selection-summary";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useHeld, useLayout } from "~/hooks/use-ring";
import { useWindowSeries } from "~/hooks/use-window-series";
import { DASHBOARD_CHART_HEIGHT } from "~/widgets/chart-frame";
import {
  PERCENT_GRID,
  PERCENT_Y_TICKS,
  windowTicks,
} from "~/widgets/lib/chart-labels";
import { StatStrip, type StatStripItem } from "~/widgets/stat-strip";
import { StreamArea } from "~/widgets/stream-area";

const LOADAVG = ["1", "5", "15"].map((w) => `cpu.loadavg{window=${w}}`);
const PSI = ["cpu.psi{stall=some}", "cpu.psi{stall=full}"];
const HELD = [
  "cpu.total",
  "cpu.user",
  "cpu.system",
  "cpu.iowait",
  "cpu.steal",
  ...LOADAVG,
  ...PSI,
];
const CHART = ["cpu.total", "cpu.system"];
const HEIGHT = DASHBOARD_CHART_HEIGHT;
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

const pct1 = (v: number | null | undefined) =>
  formatPercent(v ?? null, { decimals: 1 });

/**
 * The strip after the hero. I/O wait, steal and pressure stall are Linux
 * series (D-115), shown when the layout carries them. Idle is what neither
 * the total (user + system + steal) nor I/O wait took.
 */
function useStripItems(): { total: number | null; items: StatStripItem[] } {
  const v = useHeld(HELD);
  const layout = useLayout();
  const has = (metric: string) => layout?.byMetric.has(metric) ?? false;
  const total = v["cpu.total"] ?? null;
  const iowait = v["cpu.iowait"] ?? null;
  const idle =
    total === null || (has("cpu.iowait") && iowait === null)
      ? null
      : 100 - total - (iowait ?? 0);
  const [l1, l5, l15] = LOADAVG.map((k) => v[k] ?? null);
  const items: StatStripItem[] = [
    { label: "User", value: pct1(v["cpu.user"]) },
    { label: "System", value: pct1(v["cpu.system"]), swatch: 2 },
  ];
  if (has("cpu.iowait")) items.push({ label: "I/O wait", value: pct1(iowait) });
  if (has("cpu.steal"))
    items.push({ label: "Steal", value: pct1(v["cpu.steal"]) });
  items.push(
    { label: "Idle", value: pct1(idle), muted: true },
    {
      label: "Load avg",
      value: load(l1),
      secondary: `${load(l5)} ${load(l15)}`,
    }
  );
  if (has("cpu.psi"))
    items.push({
      label: "Pressure stall",
      value: pct1(v["cpu.psi{stall=some}"]),
      secondary: `full ${pct1(v["cpu.psi{stall=full}"])}`,
    });
  return { total, items };
}

/**
 * Stat strip over the total and system area chart (top left), over
 * the chart window (D-091). The chart is brushable (D-099): its average and
 * peak beside the live strip, and the apps table below, follow the selection.
 */
export function TotalCard({ windowMs }: { windowMs: number }) {
  const { total, items } = useStripItems();
  const series = useWindowSeries(CHART, windowMs, { brush: true });
  const brush = useAreaBrush(series, HEIGHT);
  const gaps = useGapBands("cpu");

  return (
    <div {...brushScopeProps} className="contents">
      <SectionCard
        accent="cpu"
        title="CPU total"
        hiddenTitle
        className="col-span-2"
      >
        <div className="flex flex-wrap items-end justify-between gap-x-7 gap-y-3">
          <StatStrip
            hero={{ label: "Total", value: formatPercent(total) }}
            items={items}
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
            gridlines={PERCENT_GRID}
            yTicks={PERCENT_Y_TICKS}
            xTicks={windowTicks(windowMs, 5)}
            highlight={brush.highlight}
            overlay={brush.overlay}
          />
          <SelectionSummary windowMs={windowMs} inset={32} />
        </div>
      </SectionCard>
    </div>
  );
}
