import { formatWatts } from "@core/format";
import { windowWords } from "@core/live-window";
import { useId } from "react";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useHeld } from "~/hooks/use-ring";
import { useWindowSeries } from "~/hooks/use-window-series";
import { Card } from "~/widgets/card";
import { ceilingAxis, windowTicks } from "~/widgets/lib/chart-labels";
import { StreamArea } from "~/widgets/stream-area";

const KEY = "power.gpu";

/**
 * GPU power over the chart window (D-091) on an autoscaled ceiling.
 */
export function PowerCard({ windowMs }: { windowMs: number }) {
  const titleId = useId();
  const span = `last ${windowWords(windowMs)}`;
  const now = useHeld([KEY])[KEY] ?? null;
  const series = useWindowSeries([KEY], windowMs);
  const gaps = useGapBands("gpu");
  const values = series.values[KEY] ?? [];
  const ceiling = useNiceCeiling(values, series.tEndMs, 1, windowMs);

  return (
    <Card
      accent="gpu"
      variant="chart"
      origin="tr"
      labelledBy={titleId}
      className="flex flex-col gap-3 p-4"
    >
      <div className="flex items-baseline gap-3">
        <h2 id={titleId} className="m-0 flex-1 font-[590] text-[14px]">
          Power, {span}
        </h2>
        <span className="data-mono text-[13px]">{formatWatts(now)}</span>
      </div>
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
    </Card>
  );
}
