import { formatPercent } from "@core/format";
import { windowWords } from "@core/live-window";
import { OctagonAlert, TriangleAlert } from "lucide-react";
import { useId } from "react";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useHeld } from "~/hooks/use-ring";
import { useWindowSeries } from "~/hooks/use-window-series";
import { Swap } from "~/lib/motion/swap";
import { cn } from "~/lib/utils";
import { Card } from "~/widgets/card";
import { windowTicks } from "~/widgets/lib/chart-labels";
import { StreamArea } from "~/widgets/stream-area";
import { pressureState } from "../_lib/pressure";

const Y_TICKS = [100, 75, 50, 25].map((v) => ({ value: v, label: String(v) }));

/**
 * Memory pressure over the chart window (D-091). The state word comes from
 * `mem.pressure_level`; warn and critical carry an icon so they never rely on
 * colour (design-system.md "Status colors").
 */
export function PressureCard({ windowMs }: { windowMs: number }) {
  const titleId = useId();
  const v = useHeld(["mem.pressure", "mem.pressure_level"]);
  const series = useWindowSeries(["mem.pressure"], windowMs);
  const gaps = useGapBands("memory");
  const pressure = v["mem.pressure"] ?? null;
  const state = pressureState(v["mem.pressure_level"] ?? null);

  return (
    <Card
      accent="mem"
      variant="chart"
      origin="bl"
      labelledBy={titleId}
      className="col-span-2 flex flex-col gap-3 p-4"
    >
      <div className="flex items-baseline gap-3">
        <h2 id={titleId} className="m-0 flex-1 font-[590] text-[14px]">
          Pressure
        </h2>
        {(state === "warn" || state === "critical") && (
          <Swap
            k={state}
            className={cn(
              "inline-flex items-center gap-1 text-[12px]",
              state === "warn" ? "text-warning" : "text-destructive"
            )}
          >
            {state === "warn" ? (
              <TriangleAlert
                aria-hidden
                className="size-3.5"
                strokeWidth={1.75}
              />
            ) : (
              <OctagonAlert
                aria-hidden
                className="size-3.5"
                strokeWidth={1.75}
              />
            )}
            Pressure: {state}
          </Swap>
        )}
        <span className="data-mono text-[13px]">
          {formatPercent(pressure)}
          {state && (
            <span className="text-[11px] text-muted-foreground"> {state}</span>
          )}
        </span>
      </div>
      <StreamArea
        gaps={gaps}
        series={[
          {
            key: "mem.pressure",
            values: series.values["mem.pressure"] ?? [],
            step: 1,
          },
        ]}
        tEndMs={series.tEndMs}
        intervalMs={series.intervalMs}
        yMax={100}
        accent="mem"
        height={160}
        ariaLabel={`Memory pressure, last ${windowWords(windowMs)}, now ${formatPercent(pressure)}`}
        gridlines={[25, 50, 75, 100]}
        yTicks={Y_TICKS}
        xTicks={windowTicks(windowMs, 5)}
      />
    </Card>
  );
}
