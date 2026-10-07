import { formatWatts } from "@core/format";
import { windowWords } from "@core/live-window";
import { SectionCard } from "~/components/section-card";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useHeld } from "~/hooks/use-ring";
import { useWindowSeries } from "~/hooks/use-window-series";
import { ceilingAxis, windowTicks } from "~/widgets/lib/chart-labels";
import { StreamArea } from "~/widgets/stream-area";

const KEY = "power.gpu";

/**
 * GPU power over the chart window (D-091) on an autoscaled ceiling.
 */
export function PowerCard({ windowMs }: { windowMs: number }) {
  const span = `last ${windowWords(windowMs)}`;
  const now = useHeld([KEY])[KEY] ?? null;
  const series = useWindowSeries([KEY], windowMs);
  const gaps = useGapBands("gpu");
  const values = series.values[KEY] ?? [];
  const ceiling = useNiceCeiling(values, series.tEndMs, 1, windowMs);

  return (
    <SectionCard
      accent="gpu"
      origin="tr"
      title={`Power, ${span}`}
      aside={<span className="data-mono text-[13px]">{formatWatts(now)}</span>}
      headerAlign="baseline"
      className="gap-3"
    >
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
        xTicks={windowTicks(windowMs, 3)}
      />
    </SectionCard>
  );
}
