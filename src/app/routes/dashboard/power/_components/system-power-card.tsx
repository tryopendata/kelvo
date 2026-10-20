import { formatWatts } from "@core/format";
import { windowWords } from "@core/live-window";
import { SectionCard } from "~/components/section-card";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useHeld, useLayout } from "~/hooks/use-ring";
import { useWindowSeries } from "~/hooks/use-window-series";
import { ceilingAxis, windowTicks } from "~/widgets/lib/chart-labels";
import { StreamArea } from "~/widgets/stream-area";

const KEY = "power.system";

function sensorName(key: string): string {
  return key.slice(key.indexOf("=") + 1, -1);
}

/**
 * System draw over the chart window (D-091), for a host that reports no
 * split by component (a Linux agent, D-115). Its power rails, as the kernel
 * names them (`power.sensor`), read under the chart.
 */
export function SystemPowerCard({ windowMs }: { windowMs: number }) {
  const span = `last ${windowWords(windowMs)}`;
  const sensorKeys = useLayout()?.byMetric.get("power.sensor") ?? [];
  const now = useHeld([KEY, ...sensorKeys]);
  const system = now[KEY] ?? null;
  const series = useWindowSeries([KEY], windowMs);
  const gaps = useGapBands("power");
  const values = series.values[KEY] ?? [];
  const ceiling = useNiceCeiling(values, series.tEndMs, 5, windowMs);

  return (
    <SectionCard
      accent="power"
      origin="tr"
      title={`System power, ${span}`}
      aside={<span className="figures text-[13px]">{formatWatts(system)}</span>}
    >
      <StreamArea
        gaps={gaps}
        series={[{ key: KEY, values, step: 1 }]}
        tEndMs={series.tEndMs}
        intervalMs={series.intervalMs}
        yMax={ceiling}
        accent="power"
        height={160}
        ariaLabel={`System power, ${span}, now ${formatWatts(system)}`}
        {...ceilingAxis(ceiling, `${ceiling}W`)}
        xTicks={windowTicks(windowMs, 3)}
      />
      {sensorKeys.length > 0 && (
        <div className="flex flex-wrap gap-5 border-border-subtle border-t pt-2.5">
          {sensorKeys.map((k) => (
            <span key={k} className="font-normal text-[12px] text-fg-subtle">
              {sensorName(k)}{" "}
              <span className="figures text-foreground">
                {formatWatts(now[k] ?? null)}
              </span>
            </span>
          ))}
        </div>
      )}
    </SectionCard>
  );
}
