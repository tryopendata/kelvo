import { rangeFractions } from "@core/brush";
import { fittedCeiling } from "@core/chart-math";
import {
  buildSeriesRuns,
  type ComparisonMetric,
  pointCovered,
  pointValue,
  seriesColor,
} from "@core/chart-math/series-lines";
import { formatBytes, MISSING } from "@core/format";
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
import { Link, useLocation, useNavigate } from "react-router";
import {
  BrushOverlay,
  brushScopeProps,
  useDismissOutside,
} from "~/components/brush-overlay";
import {
  ChartTooltipShell,
  SeriesSwatch,
} from "~/components/charts/chart-tooltip";
import { SectionCard } from "~/components/section-card";
import { SelectionSummary } from "~/components/selection-summary";
import { Button } from "~/components/ui/button";
import { useElementSize } from "~/hooks/use-element-size";
import { useUnits } from "~/hooks/use-units";
import {
  BrushProvider,
  useBrush,
  useBrushRange,
  useBrushStoreOptional,
} from "~/stores/brush-store";
import {
  ChartFrame,
  ChartGrid,
  ChartSelectionVeil,
  chartY,
  DASHBOARD_CHART_HEIGHT,
} from "~/widgets/chart-frame";
import { GapBand } from "~/widgets/gap-band";
import { Legend } from "~/widgets/legend";
import { historyTicks } from "~/widgets/lib/chart-labels";
import {
  containerReturnState,
  isContainerDetailClick,
} from "../_lib/navigation";
import { containerPath } from "../_lib/presentation";
import { eventCoverage } from "./history-details";

export interface ComparisonSeries {
  container: DockerContainer;
  points: readonly DockerHistoryPoint[];
  slot: number;
}
const HEIGHT = DASHBOARD_CHART_HEIGHT;
const percent = new Intl.NumberFormat("en-US", {
  maximumSignificantDigits: 3,
});
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
  coverage?: readonly DockerCoverage[];
  focused: string | null;
  pinnedFocus: string | null;
  onFocus: (id: string | null) => void;
  onPinFocus: (id: string | null) => void;
  onChoose: () => void;
  more?: number;
  chooserTitle?: string;
  loading?: boolean;
}) {
  const navigate = useNavigate();
  const location = useLocation();
  const brushStore = useBrushStoreOptional();
  const [plotRef, plotWidth] = useElementSize("width", 1000);
  const [hoverTime, setHoverTime] = useState<number | null>(null);
  const selection = useBrushRange();
  const highlight = useBrush((state) => state.draft ?? state.range);
  const pinned = selection !== null;
  const [asTable, setAsTable] = useState(false);
  const { bytes } = useUnits();
  const format = (value: number | null) =>
    metric === "cpu"
      ? value === null
        ? MISSING
        : `${percent.format(value)}%`
      : formatBytes(value, { units: bytes });
  const metricGaps = coverage.filter((gap) => !eventCoverage(gap));
  const series = sourceSeries.map((s) => ({
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
  }));
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
    () => series.flatMap((s) => s.points.map((p) => pointValue(p, metric))),
    [series, metric]
  );
  // The visible window already retains its peaks. Fit its plotted values directly:
  // holding an older ceiling hides quiet workloads and never expires when paused.
  const ceiling = fittedCeiling(
    Math.max(0, ...values.filter((value): value is number => value !== null)),
    metric === "cpu" ? 0.1 : 1024 * 1024
  );
  const active = pinnedFocus ?? focused;
  const plotSeries = series.map((s) => ({
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
  }));
  const displayName = (container: DockerContainer) =>
    `${container.name}${series.filter((s) => s.container.name === container.name).length > 1 ? ` · ${container.id.slice(0, 6)}` : ""}`;
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
    });
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
            {duration > 10_000 ? ` · peak per ${durationLabel}` : ""}
          </div>
          {asTable ? (
            <div className="h-60 overflow-auto">
              <table
                className="figures w-full text-[11px]"
                aria-label="Container history values"
              >
                <thead className="sticky top-0 bg-card">
                  <tr>
                    <th className="p-2 text-left">Time</th>
                    {series.map((s) => (
                      <th key={s.container.id} className="p-2 text-right">
                        {displayName(s.container)}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {[...times].reverse().map((time) => (
                    <tr key={time}>
                      <td className="whitespace-nowrap p-2">{clock(time)}</td>
                      {series.map((s) => {
                        const point = s.points.find((p) => p.time_ms === time);
                        const value = point ? pointValue(point, metric) : null;
                        return (
                          <td
                            key={s.container.id}
                            className="whitespace-nowrap p-2 text-right"
                          >
                            {format(value)}
                            {value !== null &&
                            point &&
                            !pointCovered(point, metric)
                              ? " partial"
                              : ""}
                          </td>
                        );
                      })}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          ) : (
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
                {plotSeries.map((s) => (
                  <g
                    key={s.container.id}
                    opacity={active && active !== s.container.id ? 0.3 : 1}
                    onPointerEnter={() => onFocus(s.container.id)}
                  >
                    {s.geometry.runs.map((run) => (
                      <path
                        key={run[0]?.time}
                        d={run
                          .map(
                            (p, index) =>
                              `${index ? "L" : "M"}${p.x},${p.y + 1}`
                          )
                          .join(" ")}
                        fill="none"
                        stroke={seriesColor(s.slot)}
                        strokeWidth={active === s.container.id ? 2 : 1}
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        vectorEffect="non-scaling-stroke"
                      />
                    ))}
                    {s.geometry.dots.map((dot) => (
                      <ellipse
                        key={dot.time}
                        cx={dot.x}
                        cy={dot.y + 1}
                        rx={(3 * 1000) / plotWidth}
                        ry={3}
                        fill={seriesColor(s.slot)}
                        vectorEffect="non-scaling-stroke"
                      />
                    ))}
                  </g>
                ))}
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
                        const point = s.points.find(
                          (p) => p.time_ms === nearest
                        );
                        const value = point ? pointValue(point, metric) : null;
                        return {
                          id: s.container.id,
                          distance:
                            value === null
                              ? Infinity
                              : Math.abs(
                                  chartY(value, ceiling, HEIGHT) -
                                    fractionY * HEIGHT
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
                <ChartTooltipShell
                  className={`top-2 ${pinned ? "pointer-events-auto" : ""} ${(selection?.fromMs ?? cursor) > (from + to) / 2 ? "left-2" : "right-2"} max-w-full text-[11px]`}
                >
                  <div>
                    {clock(cursor)}
                    {duration > 10_000
                      ? `–${clock(cursor + (rows.find((r) => r.point)?.point?.duration_ms ?? duration))}`
                      : ""}
                  </div>
                  {rows.map((row) => (
                    <div
                      key={row.container.id}
                      className="flex items-center gap-2"
                    >
                      <SeriesSwatch color={seriesColor(row.slot)} />
                      {pinned ? (
                        <Link
                          className="flex-1 underline"
                          to={link(row.container, row.point)}
                          onClick={(event) => {
                            if (!isContainerDetailClick(event)) return;
                            event.preventDefault();
                            void navigate(link(row.container, row.point), {
                              state: containerReturnState(location),
                            });
                          }}
                        >
                          {displayName(row.container)}
                        </Link>
                      ) : (
                        <span className="flex-1">
                          {displayName(row.container)}
                        </span>
                      )}
                      <span className="figures">
                        {row.value === null
                          ? "no data"
                          : row.point && row.point.duration_ms > 10_000
                            ? `${format(metric === "cpu" ? row.point.minimum.cpu_pct : row.point.minimum.working_set_bytes)} / ${format(metric === "cpu" ? row.point.metrics.cpu_pct : row.point.metrics.working_set_bytes)} / ${format(row.value)}`
                            : format(row.value)}
                        {row.point &&
                        row.value !== null &&
                        !pointCovered(row.point, metric)
                          ? " partial"
                          : ""}
                      </span>
                    </div>
                  ))}
                  {duration > 10_000 && (
                    <span className="text-muted-foreground">
                      min / avg / max
                    </span>
                  )}
                </ChartTooltipShell>
              )}
            </ChartFrame>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
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
                onPinFocus(
                  pinnedFocus === s.container.id ? null : s.container.id
                ),
            }))}
          />
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
