import { seriesRange } from "@core/chart-math";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useModuleSeries } from "~/hooks/use-module-series";
import { historyTicks } from "~/widgets/lib/chart-labels";
import { StreamArea } from "~/widgets/stream-area";
import { ChartHistoryStatus } from "./chart-history-status";

const KEYS = ["battery.charge"];

/** Recorded charge only: averaged charging booleans cannot identify charging intervals. */
export function BatteryRangeHistory() {
  const range = useChartWindow();
  return range ? (
    <ChargeHistory windowMs={range.windowMs} live={range.endMs === null} />
  ) : null;
}

function ChargeHistory({
  windowMs,
  live,
}: {
  windowMs: number;
  live: boolean;
}) {
  const series = useModuleSeries(KEYS, windowMs);
  const gaps = useGapBands("battery");
  const bounds = seriesRange(series);
  return (
    <>
      <ChartHistoryStatus loading={series.loading} error={series.error} />
      <StreamArea
        series={[
          {
            key: "battery.charge",
            values: series.values["battery.charge"] ?? [],
            step: 1,
          },
        ]}
        tEndMs={series.tEndMs}
        intervalMs={series.intervalMs}
        yMax={100}
        accent="battery"
        height={140}
        ariaLabel="Battery charge, selected range"
        ceilingLabel="100%"
        gaps={gaps}
        xTicks={historyTicks(bounds.fromMs, bounds.toMs, live, 5)}
      />
      <p className="text-[11px] text-muted-foreground">
        Recorded charge. Charging intervals are not inferred from averaged
        samples.
      </p>
    </>
  );
}
