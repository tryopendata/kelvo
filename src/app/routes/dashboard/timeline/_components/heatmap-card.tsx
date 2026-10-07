import type { HeatmapMetric } from "@core/generated/bindings";
import { historyUnavailable } from "@core/history-state";
import { memo, useCallback, useState } from "react";
import { SegmentedControl } from "~/components/segmented-control";
import { Card } from "~/widgets/card";
import { useHeatmap } from "../_hooks/use-heatmap";
import type { LaneUnits } from "../_lib/format";
import {
  cellName,
  HEATMAP_SCALES,
  type HeatmapRow,
  LEGEND_ALPHAS,
  legendEnds,
} from "../_lib/heatmap";
import { heatmapCellView, type Span } from "../_lib/time";
import { CalendarHeatmap } from "./calendar-heatmap";

const METRICS = [
  { value: "cpu", label: "Avg CPU" },
  { value: "temp", label: "Temperature" },
] as const;

const SUBTITLES: Record<HeatmapMetric, string> = {
  cpu: "Average CPU per hour.",
  temp: "Hottest SoC zone, averaged per hour.",
};

const GRID_LABELS: Record<HeatmapMetric, string> = {
  cpu: "Average CPU by hour, last 30 days",
  temp: "Average temperature by hour, last 30 days",
};

export interface HeatmapCardProps {
  units: LaneUnits;
  /** Opens the Timeline on a cell's hour. */
  onOpen: (view: { span: Span; endMs: number | null }) => void;
}

/**
 * "Last 30 days, by hour": the calendar heatmap with
 * its metric toggle and legend. Reads once an hour and never subscribes to
 * the live frames.
 */
export const HeatmapCard = memo(function HeatmapCard({
  units,
  onOpen,
}: HeatmapCardProps) {
  const [metric, setMetric] = useState<HeatmapMetric>("cpu");
  const { rows, current, hourMs, loading, error } = useHeatmap(metric);
  const scale = HEATMAP_SCALES[metric];
  const [lo, hi] = legendEnds(metric, units);

  const name = useCallback(
    (row: HeatmapRow, hour: number) =>
      cellName(row, hour, metric, units, { nowHourMs: hourMs, loading }),
    [metric, units, hourMs, loading]
  );
  const select = useCallback(
    (row: HeatmapRow, hour: number) =>
      onOpen(
        heatmapCellView(
          row.hourStarts[hour] as number,
          row.hourStarts[hour + 1] as number,
          Date.now()
        )
      ),
    [onOpen]
  );

  return (
    <Card
      accent={scale.accent}
      origin="br"
      variant="chart"
      ariaLabel="30-day heatmap"
      className="flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex min-w-[220px] flex-1 flex-col gap-0.5">
          <h2 className="font-[590] text-[14px]">Last 30 days, by hour</h2>
          <span className="font-normal text-[12px] text-muted-foreground">
            {SUBTITLES[metric]} Hatched hours had no samples (asleep or off).
            Click a cell to open that hour.
          </span>
        </div>
        <SegmentedControl
          options={METRICS}
          value={metric}
          onChange={setMetric}
          ariaLabel="Heatmap metric"
        />
        <div aria-hidden className="flex items-center gap-1">
          <span className="data-mono mr-1 text-[10px] text-muted-foreground">
            {lo}
          </span>
          {LEGEND_ALPHAS.map((a) => (
            <span
              key={a}
              data-legend-swatch
              className="h-2.5 w-3.5 rounded-[2px]"
              style={{
                background: `color-mix(in srgb, var(--color-${scale.accent}) ${a * 100}%, transparent)`,
              }}
            />
          ))}
          <span className="data-mono ml-1 text-[10px] text-muted-foreground">
            {hi}
          </span>
        </div>
      </div>
      {error ? (
        <p className="font-normal text-[13px] text-fg-subtle">
          {historyUnavailable(error.error)
            ? "No history is being kept, so there are no past hours to show."
            : `The heatmap could not be read (${error.message}).`}
        </p>
      ) : (
        <CalendarHeatmap
          rows={rows}
          scale={scale}
          current={current}
          nowHourMs={hourMs}
          loading={loading}
          cellName={name}
          onSelect={select}
          ariaLabel={GRID_LABELS[metric]}
        />
      )}
    </Card>
  );
});
