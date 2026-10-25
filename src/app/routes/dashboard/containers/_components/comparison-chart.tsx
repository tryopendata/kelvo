import {
  type ComparisonMetric,
  pointValue,
  seriesColor,
} from "@core/chart-math/series-lines";
import { formatBytes, formatPercent } from "@core/format";
import type {
  DockerContainer,
  DockerCoverage,
  DockerHistoryPoint,
} from "@core/generated/bindings";
import {
  type ComponentProps,
  type ReactNode,
  useCallback,
  useMemo,
  useState,
} from "react";
import { brushScopeProps } from "~/components/brush-overlay";
import { SectionCard } from "~/components/section-card";
import { SelectionSummary } from "~/components/selection-summary";
import { Button } from "~/components/ui/button";
import { useUnits } from "~/hooks/use-units";
import { BrushProvider, useBrushStoreOptional } from "~/stores/brush-store";
import { Legend } from "~/widgets/legend";
import { ComparisonCanvas } from "./comparison-canvas";
import { useComparisonPlotState } from "./comparison-plot-state";

import { ComparisonValues } from "./comparison-values";
import { eventCoverage } from "./history-details";
export interface ComparisonSeries {
  container: DockerContainer;
  points: readonly DockerHistoryPoint[];
  slot: number;
}
export function ComparisonChart(props: ComponentProps<typeof ComparisonPlot>) {
  const provider = useBrushStoreOptional();
  return provider ? (
    <ComparisonPlot {...props} />
  ) : (
    <BrushProvider>
      <ComparisonPlot {...props} />
    </BrushProvider>
  );
}
function ComparisonPlot({
  series: sourceSeries,
  metric,
  from,
  to,
  live,
  range,
  controls,
  notice,
  coverage = [],
  focused,
  pinnedFocus,
  onFocus,
  onPinFocus,
  onChoose,
  more = 0,
  chooserTitle,
  loading = false,
}: {
  series: readonly ComparisonSeries[];
  metric: ComparisonMetric;
  from: number;
  to: number;
  live: boolean;
  range: string;
  controls?: ReactNode;
  notice?: ReactNode;
  coverage?: readonly DockerCoverage[] | undefined;
  focused: string | null;
  pinnedFocus: string | null;
  onFocus: (id: string | null) => void;
  onPinFocus: (id: string | null) => void;
  onChoose: () => void;
  more?: number;
  chooserTitle?: string | undefined;
  loading?: boolean;
}) {
  const [asTable, setAsTable] = useState(false);
  const { bytes } = useUnits();
  const format = (value: number | null) =>
    metric === "cpu"
      ? formatPercent(value, { significantDigits: 3 })
      : formatBytes(value, { units: bytes });
  const statistics = (point: DockerHistoryPoint) =>
    (["minimum", "average", "maximum"] as const)
      .map((statistic) => format(pointValue(point, metric, statistic)))
      .join(" / ");
  const metricGaps = useMemo(
    () => coverage.filter((gap) => !eventCoverage(gap)),
    [coverage]
  );
  const series = useMemo(
    () =>
      sourceSeries.map((s) => ({
        ...s,
        points: s.points
          .filter((point) => point.time_ms >= from && point.time_ms < to)
          .map((point) =>
            metricGaps.some(
              (gap) =>
                (gap.container_id === null ||
                  gap.container_id === s.container.id) &&
                gap.start_ms < point.time_ms + point.duration_ms &&
                (gap.end_ms ?? to) > point.time_ms
            )
              ? { ...point, cpu_measured_ms: 0, memory_measured_ms: 0 }
              : point
          ),
      })),
    [sourceSeries, from, to, metricGaps]
  );
  const displayName = useCallback(
    (container: DockerContainer) =>
      `${container.name}${series.filter((s) => s.container.name === container.name).length > 1 ? ` · ${container.id.slice(0, 6)}` : ""}`,
    [series]
  );
  const plot = useComparisonPlotState(
    series,
    metric,
    from,
    to,
    focused,
    pinnedFocus,
    onFocus,
    onPinFocus
  );
  const { selection, times } = plot;
  const duration = Math.max(
    10_000,
    ...series.flatMap((s) => s.points.map((p) => p.duration_ms))
  );
  const durationLabel =
    duration < 60_000
      ? `${duration / 1000} s`
      : duration < 3_600_000
        ? `${duration / 60_000} min`
        : `${duration / 3_600_000} h`;
  const clock = (time: number) =>
    new Date(time).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: duration < 60_000 ? "2-digit" : undefined,
    });
  const legend = useMemo(
    () => (
      <Legend
        items={series.map((s) => ({
          label: displayName(s.container),
          value: "",
          step: "none",
          color: seriesColor(s.slot),
          pressed: pinnedFocus === s.container.id,
          onFocus: () => onFocus(s.container.id),
          onBlur: () => onFocus(null),
          onClick: () =>
            onPinFocus(pinnedFocus === s.container.id ? null : s.container.id),
        }))}
      />
    ),
    [series, displayName, pinnedFocus, onFocus, onPinFocus]
  );
  return (
    <div {...brushScopeProps}>
      <SectionCard
        accent={metric === "cpu" ? "cpu" : "mem"}
        title={`${metric === "cpu" ? "CPU" : "Memory"} by container`}
        aside={controls}
      >
        <div className="min-w-0">
          <div className="mb-1.5 text-[11px] text-muted-foreground">
            {metric === "cpu" ? "% of one core" : "Working set"}
            {` · ${durationLabel} average · shaded min–max`}
          </div>
          {asTable ? (
            <ComparisonValues
              series={series}
              times={times}
              metric={metric}
              displayName={displayName}
              clock={clock}
              statistics={statistics}
              format={format}
            />
          ) : (
            <ComparisonCanvas
              series={series}
              metric={metric}
              from={from}
              to={to}
              live={live}
              range={range}
              metricGaps={metricGaps}
              plot={plot}
              onFocus={onFocus}
              loading={loading}
              duration={duration}
              clock={clock}
              displayName={displayName}
              statistics={statistics}
              format={format}
            />
          )}
        </div>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          {legend}
          <Button
            variant="ghost"
            size="sm"
            title={chooserTitle}
            onClick={onChoose}
          >
            {more > 0 ? `+${more} more` : "Choose…"}
          </Button>
          <button
            type="button"
            className="ml-auto w-fit rounded-control text-[11px] text-muted-foreground underline underline-offset-2 focus-visible:ring-2 focus-visible:ring-ring"
            onClick={() => setAsTable((value) => !value)}
          >
            {asTable ? "Show as chart" : "Show as table"}
          </button>
        </div>

        {selection && (
          <SelectionSummary windowMs={to - from} fromMs={from} inset={48} />
        )}
        {notice}
      </SectionCard>
    </div>
  );
}
