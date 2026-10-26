import { seriesRange } from "@core/chart-math";
import { formatPercent } from "@core/format";
import { windowWords } from "@core/live-window";
import { OctagonAlert, TriangleAlert } from "lucide-react";
import { useAreaBrush } from "~/components/area-brush";
import { brushScopeProps } from "~/components/brush-overlay";
import { ChartHistoryStatus } from "~/components/chart-history-status";
import { SectionCard } from "~/components/section-card";
import { SelectionSummary } from "~/components/selection-summary";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useModuleSeries } from "~/hooks/use-module-series";
import { useHeld } from "~/hooks/use-ring";
import { Swap } from "~/lib/motion/swap";
import { cn } from "~/lib/utils";
import { useModuleRange } from "~/stores/chart-range";
import {
  historyTicks,
  PERCENT_GRID,
  PERCENT_Y_TICKS,
} from "~/widgets/lib/chart-labels";
import { StreamArea } from "~/widgets/stream-area";
import { pressureState } from "../_lib/pressure";

const HEIGHT = 160;
const KEY = "mem.pressure";
const KEYS = [KEY];

/**
 * Memory pressure over the chart window (D-091). The state word comes from
 * `mem.pressure_level`; warn and critical carry an icon so they never rely on
 * colour (design-system.md "Status colors"). Brushable (D-099): a range
 * scopes the peaks and the apps table. A Linux host has no macOS pressure;
 * its page shows `UsedCard` here instead.
 */
export function PressureCard({ windowMs }: { windowMs: number }) {
  const v = useHeld([...KEYS, "mem.pressure_level"]);
  const endMs = useModuleRange()?.endMs ?? null;
  const series = useModuleSeries(KEYS, windowMs, { brush: true });
  const gaps = useGapBands("memory");
  const brush = useAreaBrush(series, HEIGHT, true, gaps);
  const pressure = v[KEY] ?? null;
  const state = pressureState(v["mem.pressure_level"] ?? null);

  return (
    <div {...brushScopeProps} className="contents">
      <SectionCard
        accent="mem"
        origin="bl"
        title="Pressure"
        headerAlign="baseline"
        compactHeader
        headerWrap={false}
        className="gap-3"
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
              {formatPercent(pressure)}
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
          <ChartHistoryStatus loading={series.loading} error={series.error} />
          <StreamArea
            gaps={gaps}
            series={[{ key: KEY, values: series.values[KEY] ?? [], step: 1 }]}
            tEndMs={series.tEndMs}
            intervalMs={series.intervalMs}
            yMax={100}
            accent="mem"
            height={HEIGHT}
            ariaLabel={`Memory pressure, ${endMs === null ? `last ${windowWords(windowMs)}` : "selected dates"}, now ${formatPercent(pressure)}. Drag to select a range.`}
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
