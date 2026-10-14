import { rangeFractions } from "@core/brush";
import {
  type ComparisonMetric,
  pointValue,
} from "@core/chart-math/series-lines";
import type {
  DockerContainer,
  DockerCoverage,
  DockerHistoryPoint,
} from "@core/generated/bindings";
import { BrushOverlay } from "~/components/brush-overlay";
import {
  ChartFrame,
  ChartGrid,
  ChartSelectionVeil,
  chartY,
  DASHBOARD_CHART_HEIGHT,
} from "~/widgets/chart-frame";
import { GapBand } from "~/widgets/gap-band";
import { historyTicks } from "~/widgets/lib/chart-labels";
import type { ComparisonSeries } from "./comparison-chart";
import { ComparisonLines } from "./comparison-lines";
import type { ComparisonPlotState } from "./comparison-plot-state";
import { ComparisonTooltip } from "./comparison-tooltip";

const HEIGHT = DASHBOARD_CHART_HEIGHT;
export function ComparisonCanvas({
  series,
  metric,
  from,
  to,
  live,
  range,
  metricGaps,
  plot,
  onFocus,
  loading,
  duration,
  clock,
  displayName,
  statistics,
  format,
}: {
  series: readonly ComparisonSeries[];
  metric: ComparisonMetric;
  from: number;
  to: number;
  live: boolean;
  range: string;
  metricGaps: readonly DockerCoverage[];
  plot: ComparisonPlotState;
  onFocus: (id: string | null) => void;
  loading: boolean;
  duration: number;
  clock: (time: number) => string;
  displayName: (container: DockerContainer) => string;
  statistics: (point: DockerHistoryPoint) => string;
  format: (value: number | null) => string;
}) {
  const {
    brushStore,
    plotRef,
    plotWidth,
    setHoverTime,
    selection,
    highlight,
    pinned,
    times,
    cursor,
    ceiling,
    active,
    plotSeries,
    rows,
    link,
  } = plot;
  return (
    <ChartFrame
      height={HEIGHT}
      ceiling={ceiling}
      gutter={48}
      plotRef={plotRef}
      yTicks={[ceiling, ceiling / 2, 0].map((value) => ({
        value,
        label: value === 0 ? "0" : format(value),
      }))}
      xTicks={historyTicks(from, to, live)}
    >
      <div className="pointer-events-none absolute inset-0">
        {metricGaps
          .filter((gap) => gap.container_id === null)
          .map((gap) => (
            <GapBand
              key={`${gap.reason}:${gap.start_ms}`}
              fromMs={gap.start_ms}
              toMs={gap.end_ms ?? to}
              rangeFromMs={from}
              rangeToMs={to}
              label={gap.reason}
              labelPosition="none"
            />
          ))}
      </div>
      <svg
        role="img"
        aria-label={`${metric === "cpu" ? "CPU" : "Memory"} by container, ${range}, ${series.map((s) => displayName(s.container)).join(", ") || "no containers"}`}
        viewBox={`0 0 1000 ${HEIGHT}`}
        preserveAspectRatio="none"
        height={HEIGHT}
        className="block w-full overflow-visible rounded-control focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <ChartGrid
          values={[ceiling / 2, ceiling]}
          ceiling={ceiling}
          height={HEIGHT}
        />
        <ComparisonLines
          plotSeries={plotSeries}
          active={active}
          onFocus={onFocus}
          plotWidth={plotWidth}
        />
        {cursor !== null && (
          <line
            x1={((cursor - from) / (to - from)) * 1000}
            x2={((cursor - from) / (to - from)) * 1000}
            y1={0}
            y2={HEIGHT}
            stroke="var(--color-muted-foreground)"
            vectorEffect="non-scaling-stroke"
          />
        )}
      </svg>
      <ChartSelectionVeil
        highlight={
          highlight ? rangeFractions(highlight, from, to - from) : null
        }
      />
      {brushStore && (
        <BrushOverlay
          firstMs={from}
          spanMs={to - from}
          height={HEIGHT}
          inset={0}
          onHoverTime={(time, fractionY) => {
            if (time === null || !times.length) {
              setHoverTime(null);
              onFocus(null);
              return;
            }
            const nearest = times.reduce((a, b) =>
              Math.abs(a - time) < Math.abs(b - time) ? a : b
            );
            setHoverTime(time);
            const line = series
              .map((s) => {
                const point = s.points.find((p) => p.time_ms === nearest);
                const value = point ? pointValue(point, metric) : null;
                return {
                  id: s.container.id,
                  distance:
                    value === null
                      ? Infinity
                      : Math.abs(
                          chartY(value, ceiling, HEIGHT) - fractionY * HEIGHT
                        ),
                };
              })
              .sort((a, b) => a.distance - b.distance)[0];
            onFocus(line && line.distance < 8 ? line.id : null);
          }}
        />
      )}
      {!times.length && (
        <p className="pointer-events-none absolute inset-0 flex items-center justify-center text-[12px] text-muted-foreground">
          {loading
            ? "Loading recorded measurements…"
            : "No recorded measurements in this range."}
        </p>
      )}
      {cursor !== null && (
        <ComparisonTooltip
          rows={rows}
          pinned={pinned}
          selection={selection}
          cursor={cursor}
          from={from}
          to={to}
          duration={duration}
          clock={clock}
          link={link}
          displayName={displayName}
          statistics={statistics}
          format={format}
          metric={metric}
        />
      )}
    </ChartFrame>
  );
}
