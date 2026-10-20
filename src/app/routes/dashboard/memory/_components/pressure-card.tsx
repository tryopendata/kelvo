import { formatPercent } from "@core/format";
import { windowWords } from "@core/live-window";
import { OctagonAlert, TriangleAlert } from "lucide-react";
import { useAreaBrush } from "~/components/area-brush";
import { brushScopeProps } from "~/components/brush-overlay";
import { SectionCard } from "~/components/section-card";
import { SelectionSummary } from "~/components/selection-summary";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useHeld, useLayout } from "~/hooks/use-ring";
import { useWindowSeries } from "~/hooks/use-window-series";
import { Swap } from "~/lib/motion/swap";
import { cn } from "~/lib/utils";
import {
  PERCENT_GRID,
  PERCENT_Y_TICKS,
  windowTicks,
} from "~/widgets/lib/chart-labels";
import { StreamArea } from "~/widgets/stream-area";
import { pressureState } from "../_lib/pressure";

const HEIGHT = 160;
const PRESSURE = ["mem.pressure"];
const PSI = ["mem.psi{stall=some}", "mem.psi{stall=full}"];

/**
 * A Linux agent reports pressure stall information (`mem.psi`, D-115) in
 * place of macOS memory pressure: the share of time some, and all, tasks
 * waited on memory.
 */
function usePsi(): boolean {
  const layout = useLayout();
  return (
    (layout?.byMetric.has("mem.psi") ?? false) &&
    !(layout?.byMetric.has("mem.pressure") ?? false)
  );
}

/**
 * Memory pressure over the chart window (D-091). The state word comes from
 * `mem.pressure_level`; warn and critical carry an icon so they never rely on
 * colour (design-system.md "Status colors"). Brushable (D-099): a range
 * scopes the peaks and the apps table.
 */
export function PressureCard({ windowMs }: { windowMs: number }) {
  const psi = usePsi();
  const keys = psi ? PSI : PRESSURE;
  const v = useHeld([...keys, "mem.pressure_level"]);
  const series = useWindowSeries(keys, windowMs, { brush: true });
  const brush = useAreaBrush(series, HEIGHT);
  const gaps = useGapBands("memory");
  const pressure = v[psi ? "mem.psi{stall=some}" : "mem.pressure"] ?? null;
  const state = psi ? null : pressureState(v["mem.pressure_level"] ?? null);
  const shown = (x: number | null) =>
    formatPercent(x, psi ? { decimals: 1 } : {});

  return (
    <div {...brushScopeProps} className="contents">
      <SectionCard
        accent="mem"
        origin="bl"
        title={psi ? "Pressure stall" : "Pressure"}
        headerAlign="baseline"
        compactHeader
        headerWrap={false}
        className="col-span-2 gap-3"
        aside={
          <span className="flex items-baseline gap-3">
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
            <span className="figures text-[13px]">
              {shown(pressure)}
              {state && (
                <span className="text-[11px] text-muted-foreground">
                  {" "}
                  {state}
                </span>
              )}
            </span>
          </span>
        }
      >
        <div className="flex flex-col gap-1.5">
          <StreamArea
            gaps={gaps}
            series={keys.map((key, i) => ({
              key,
              values: series.values[key] ?? [],
              step: i === 0 ? 1 : 2,
            }))}
            tEndMs={series.tEndMs}
            intervalMs={series.intervalMs}
            yMax={100}
            accent="mem"
            height={HEIGHT}
            ariaLabel={`Memory ${psi ? "pressure stall" : "pressure"}, last ${windowWords(windowMs)}, now ${shown(pressure)}. Drag to select a range.`}
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
