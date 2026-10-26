import { seriesRange } from "@core/chart-math";
import { formatGhz, formatPercent, formatWatts } from "@core/format";
import { windowWords } from "@core/live-window";
import { useAreaBrush } from "~/components/area-brush";
import { brushScopeProps } from "~/components/brush-overlay";
import { ChartHistoryStatus } from "~/components/chart-history-status";
import {
  type RangeTotal,
  RangeTotalsStrip,
} from "~/components/range-totals-strip";
import { SectionCard } from "~/components/section-card";
import { SelectionSummary } from "~/components/selection-summary";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useModuleSeries } from "~/hooks/use-module-series";
import { useHeld } from "~/hooks/use-ring";
import { useModuleRange } from "~/stores/chart-range";
import {
  historyTicks,
  PERCENT_GRID,
  PERCENT_Y_TICKS,
} from "~/widgets/lib/chart-labels";
import { StatStrip } from "~/widgets/stat-strip";
import { StreamArea } from "~/widgets/stream-area";

const HELD = ["gpu.util", "gpu.render", "gpu.tiler", "gpu.freq", "power.gpu"];
const CHART = ["gpu.util"];
const HEIGHT = 200;
/** Over the window or the selection (D-099): `gpu.util` averaged and its peak. */
const TOTALS: readonly RangeTotal[] = [
  {
    metric: "gpu.util",
    label: "Avg",
    format: (s) => formatPercent(s.avg, { decimals: 1 }),
  },
  { metric: "gpu.util", label: "Peak", format: (s) => formatPercent(s.max) },
];

/**
 * Utilization strip over the utilization chart, the GPU page's lead card,
 * over the chart window (D-091). Brushable (D-099): the average and peak
 * beside the live strip, and the apps table, follow the selection.
 */
export function UsageCard({ windowMs }: { windowMs: number }) {
  const v = useHeld(HELD);
  const endMs = useModuleRange()?.endMs ?? null;
  const series = useModuleSeries(CHART, windowMs, { brush: true });
  const gaps = useGapBands("gpu");
  const brush = useAreaBrush(series, HEIGHT, true, gaps);
  const util = v["gpu.util"] ?? null;

  return (
    <div {...brushScopeProps} className="contents">
      <SectionCard accent="gpu" title="GPU utilization" hiddenTitle>
        <div className="flex flex-wrap items-end justify-between gap-x-7 gap-y-3">
          <StatStrip
            hero={{ label: "Utilization", value: formatPercent(util) }}
            items={[
              { label: "Renderer", value: formatPercent(v["gpu.render"]) },
              { label: "Tiler", value: formatPercent(v["gpu.tiler"]) },
              {
                label: "Frequency",
                value: formatGhz(v["gpu.freq"], { decimals: 2 }),
              },
              { label: "Power", value: formatWatts(v["power.gpu"]) },
            ]}
          />
          <RangeTotalsStrip
            windowMs={windowMs}
            totals={TOTALS}
            testId="gpu-range-totals"
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <ChartHistoryStatus loading={series.loading} error={series.error} />
          <StreamArea
            gaps={gaps}
            series={[
              {
                key: "gpu.util",
                values: series.values["gpu.util"] ?? [],
                step: 1,
              },
            ]}
            tEndMs={series.tEndMs}
            intervalMs={series.intervalMs}
            yMax={100}
            accent="gpu"
            height={HEIGHT}
            ariaLabel={`GPU utilization, ${endMs === null ? `last ${windowWords(windowMs)}` : "selected dates"}, now ${formatPercent(util)}. Drag to select a range.`}
            gridlines={PERCENT_GRID}
            yTicks={PERCENT_Y_TICKS}
            xTicks={historyTicks(
              seriesRange(series).fromMs,
              seriesRange(series).toMs,
              endMs === null,
              5
            )}
            highlight={brush.highlight}
            overlay={brush.overlay}
          />
          <SelectionSummary windowMs={windowMs} inset={32} />
        </div>
      </SectionCard>
    </div>
  );
}
