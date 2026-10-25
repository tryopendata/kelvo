import { fittedCeiling } from "@core/chart-math";
import {
  buildSeriesRuns,
  type ComparisonMetric,
  pointValue,
} from "@core/chart-math/series-lines";
import type {
  DockerContainer,
  DockerHistoryPoint,
} from "@core/generated/bindings";
import { useCallback, useMemo, useState } from "react";
import { useDismissOutside } from "~/components/brush-overlay";
import { useElementSize } from "~/hooks/use-element-size";
import {
  useBrush,
  useBrushRange,
  useBrushStoreOptional,
} from "~/stores/brush-store";
import { DASHBOARD_CHART_HEIGHT } from "~/widgets/chart-frame";
import { containerPath } from "../_lib/presentation";
import type { ComparisonSeries } from "./comparison-chart";

const HEIGHT = DASHBOARD_CHART_HEIGHT;
export function useComparisonPlotState(
  series: readonly ComparisonSeries[],
  metric: ComparisonMetric,
  from: number,
  to: number,
  focused: string | null,
  pinnedFocus: string | null,
  onFocus: (id: string | null) => void,
  onPinFocus: (id: string | null) => void
) {
  const brushStore = useBrushStoreOptional();
  const [plotRef, plotWidth] = useElementSize("width", 1000);
  const [hoverTime, setHoverTime] = useState<number | null>(null);
  const selection = useBrushRange();
  const highlight = useBrush((state) => state.draft ?? state.range);
  const pinned = selection !== null;
  const times = useMemo(
    () =>
      [...new Set(series.flatMap((s) => s.points.map((p) => p.time_ms)))]
        .filter((t) => t >= from && t < to)
        .sort((a, b) => a - b),
    [series, from, to]
  );
  const time = hoverTime ?? selection?.fromMs ?? null;
  const cursor =
    time === null || !times.length
      ? null
      : times.reduce((a, b) =>
          Math.abs(a - time) < Math.abs(b - time) ? a : b
        );
  const values = useMemo(
    () =>
      series.flatMap((s) =>
        s.points.flatMap((p) => [
          pointValue(p, metric),
          pointValue(p, metric, "maximum"),
        ])
      ),
    [series, metric]
  );
  // The visible window already retains its peaks. Fit its plotted values directly:
  // holding an older ceiling hides quiet workloads and never expires when paused.
  const ceiling = fittedCeiling(
    Math.max(0, ...values.filter((value): value is number => value !== null)),
    metric === "cpu" ? 0.1 : 1024 * 1024
  );
  const active = pinnedFocus ?? focused;
  const plotSeries = useMemo(
    () =>
      series.map((s) => ({
        ...s,
        geometry: buildSeriesRuns(
          s.points,
          metric,
          from,
          to,
          1000,
          HEIGHT - 2,
          ceiling
        ),
      })),
    [series, metric, from, to, ceiling]
  );
  const rows = series
    .map((s) => {
      const point = s.points.find((p) => p.time_ms === cursor);
      return { ...s, point, value: point ? pointValue(point, metric) : null };
    })
    .sort((a, b) =>
      pinned
        ? a.slot - b.slot
        : (b.value ?? -Infinity) - (a.value ?? -Infinity) ||
          a.container.id.localeCompare(b.container.id)
    );
  const link = (container: DockerContainer, point?: DockerHistoryPoint) =>
    `${containerPath(container.engine_id, container.id)}?metric=${metric}&range=15m&end=${(point ? point.time_ms + point.duration_ms : (cursor ?? to)) + 300_000}`;
  const dismiss = useCallback(() => {
    setHoverTime(null);
    onPinFocus(null);
    onFocus(null);
  }, [onPinFocus, onFocus]);
  useDismissOutside(dismiss, hoverTime !== null || pinnedFocus !== null);

  return {
    brushStore,
    plotRef,
    plotWidth,
    hoverTime,
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
  };
}
export type ComparisonPlotState = ReturnType<typeof useComparisonPlotState>;
