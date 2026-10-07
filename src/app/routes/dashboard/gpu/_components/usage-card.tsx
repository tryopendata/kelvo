import { formatGhz, formatPercent, formatWatts } from "@core/format";
import { windowWords } from "@core/live-window";
import { useId } from "react";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useHeld } from "~/hooks/use-ring";
import { useWindowSeries } from "~/hooks/use-window-series";
import { Card } from "~/widgets/card";
import { windowTicks } from "~/widgets/lib/chart-labels";
import { StatStrip } from "~/widgets/stat-strip";
import { StreamArea } from "~/widgets/stream-area";

const HELD = ["gpu.util", "gpu.render", "gpu.tiler", "gpu.freq", "power.gpu"];
const CHART = ["gpu.util"];
const Y_TICKS = [100, 75, 50, 25].map((v) => ({ value: v, label: String(v) }));

/**
 * Utilization strip over the utilization chart, the GPU page's lead card,
 * over the chart window (D-091).
 */
export function UsageCard({ windowMs }: { windowMs: number }) {
  const titleId = useId();
  const v = useHeld(HELD);
  const series = useWindowSeries(CHART, windowMs);
  const gaps = useGapBands("gpu");
  const util = v["gpu.util"] ?? null;

  return (
    <Card
      accent="gpu"
      variant="chart"
      labelledBy={titleId}
      className="flex flex-col gap-3.5 p-4"
    >
      <h2 id={titleId} className="sr-only">
        GPU utilization
      </h2>
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
      <StreamArea
        gaps={gaps}
        series={[
          { key: "gpu.util", values: series.values["gpu.util"] ?? [], step: 1 },
        ]}
        tEndMs={series.tEndMs}
        intervalMs={series.intervalMs}
        yMax={100}
        accent="gpu"
        height={200}
        ariaLabel={`GPU utilization, last ${windowWords(windowMs)}, now ${formatPercent(util)}`}
        gridlines={[25, 50, 75, 100]}
        yTicks={Y_TICKS}
        xTicks={windowTicks(windowMs, 5)}
      />
    </Card>
  );
}
