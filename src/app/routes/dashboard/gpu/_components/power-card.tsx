import { seriesRange } from "@core/chart-math";
import { formatWatts } from "@core/format";
import { windowWords } from "@core/live-window";
import { ChartHistoryStatus } from "~/components/chart-history-status";
import { SectionCard } from "~/components/section-card";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useModuleSeries } from "~/hooks/use-module-series";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useHeld } from "~/hooks/use-ring";
import { useModuleRange } from "~/stores/chart-range";
import { ceilingAxis, historyTicks } from "~/widgets/lib/chart-labels";
import { StreamArea } from "~/widgets/stream-area";

const KEY = "power.gpu";

/**
 * GPU power over the chart window (D-091) on an autoscaled ceiling.
 */
export function PowerCard({ windowMs }: { windowMs: number }) {
  const now = useHeld([KEY])[KEY] ?? null;
  const endMs = useModuleRange()?.endMs ?? null;
  const span =
    endMs === null ? `last ${windowWords(windowMs)}` : "selected dates";
  const series = useModuleSeries([KEY], windowMs);
  const gaps = useGapBands("gpu");
  const values = series.values[KEY] ?? [];
  const ceiling = useNiceCeiling(values, series.tEndMs, 1, windowMs);

  return (
    <SectionCard
      accent="gpu"
      origin="tr"
      title={`Power, ${span}`}
      aside={<span className="figures text-[13px]">{formatWatts(now)}</span>}
      headerAlign="baseline"
      compactHeader
      headerWrap={false}
      className="gap-3"
    >
      <ChartHistoryStatus loading={series.loading} error={series.error} />
      <StreamArea
        gaps={gaps}
        series={[{ key: KEY, values, step: 1 }]}
        tEndMs={series.tEndMs}
        intervalMs={series.intervalMs}
        yMax={ceiling}
        accent="gpu"
        height={140}
        ariaLabel={`GPU power, ${span}, now ${formatWatts(now)}`}
        {...ceilingAxis(ceiling, `${ceiling}W`)}
        xTicks={historyTicks(
          seriesRange(series).fromMs,
          seriesRange(series).toMs,
          endMs === null,
          3
        )}
      />
    </SectionCard>
  );
}
