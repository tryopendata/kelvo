import {
  buildSeriesRuns,
  type ComparisonMetric,
  pointCovered,
  pointValue,
  seriesColor,
} from "@core/chart-math/series-lines";
import { formatBytes, formatPercent } from "@core/format";
import type {
  DockerContainer,
  DockerCoverage,
  DockerHistoryPoint,
} from "@core/generated/bindings";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router";
import { BrushOverlay } from "~/components/brush-overlay";
import {
  ChartTooltipShell,
  SeriesSwatch,
} from "~/components/charts/chart-tooltip";
import { SectionCard } from "~/components/section-card";
import { Button } from "~/components/ui/button";
import { useElementSize } from "~/hooks/use-element-size";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useUnits } from "~/hooks/use-units";
import { useBrushStoreOptional } from "~/stores/brush-store";
import { GapBand } from "~/widgets/gap-band";
import { Legend } from "~/widgets/legend";
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
export function ComparisonChart({
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
  const root = useRef<HTMLDivElement>(null);
  const [plotRef, plotWidth] = useElementSize("width", 1000);
  const [cursor, setCursor] = useState<number | null>(null);
  const [pinned, setPinned] = useState(false);
  const [asTable, setAsTable] = useState(false);
  const { bytes } = useUnits();
  const format = (value: number | null) =>
    metric === "cpu"
      ? formatPercent(value)
      : formatBytes(value, { units: bytes });
  const metricGaps = coverage.filter((gap) => !eventCoverage(gap));
  const series = sourceSeries.map((s) => ({
    ...s,
    points: s.points.map((point) =>
      metricGaps.some(
        (gap) =>
          (gap.container_id === null || gap.container_id === s.container.id) &&
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
  const values = useMemo(
    () => series.flatMap((s) => s.points.map((p) => pointValue(p, metric))),
    [series, metric]
  );
  const ceiling = useNiceCeiling(
    values,
    times.at(-1) ?? 0,
    metric === "cpu" ? 10 : 64 * 1024 * 1024,
    to - from
  );
  const active = pinnedFocus ?? focused;
  const plotSeries = series.map((s) => ({
    ...s,
    geometry: buildSeriesRuns(s.points, metric, from, to, 1000, 236, ceiling),
  }));
  const displayName = (container: DockerContainer) =>
    `${container.name}${series.filter((s) => s.container.name === container.name).length > 1 ? ` · ${container.id.slice(0, 6)}` : ""}`;
  const rows = series
    .map((s) => {
      const point = s.points.find((p) => p.time_ms === cursor);
      return { ...s, point, value: point ? pointValue(point, metric) : null };
    })
    .sort(
      (a, b) =>
        (b.value ?? -Infinity) - (a.value ?? -Infinity) ||
        a.container.id.localeCompare(b.container.id)
    );
  const link = (container: DockerContainer, point?: DockerHistoryPoint) =>
    `${containerPath(container.engine_id, container.id)}?metric=${metric}&range=15m&end=${(point ? point.time_ms + point.duration_ms : (cursor ?? to)) + 300_000}`;
  useEffect(() => {
    const dismiss = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setPinned(false);
        setCursor(null);
        onPinFocus(null);
        onFocus(null);
      }
    };
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setPinned(false);
    };
    document.addEventListener("keydown", dismiss);
    document.addEventListener("pointerdown", outside);
    return () => {
      document.removeEventListener("keydown", dismiss);
      document.removeEventListener("pointerdown", outside);
    };
  }, [onPinFocus, onFocus]);
  useEffect(() => {
    if (live) {
      setPinned(false);
      setCursor(null);
    }
  }, [live]);
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
    <SectionCard
      className="gap-1 py-3"
      accent={metric === "cpu" ? "cpu" : "mem"}
      title={`${metric === "cpu" ? "CPU" : "Memory"} by container`}
      aside={controls}
    >
      <div ref={root} className="min-w-0">
        <div className="mb-1 text-[11px] text-muted-foreground">
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
          <div className="relative h-60 pl-12">
            <div
              aria-hidden
              className="figures absolute inset-y-0 left-0 flex w-11 flex-col justify-between pr-2 text-right text-[10px] text-muted-foreground"
            >
              <span>{format(ceiling)}</span>
              <span>{format(ceiling / 2)}</span>
              <span>0</span>
            </div>
            <div ref={plotRef} className="relative h-full">
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
              {/* biome-ignore lint/a11y/noNoninteractiveElementInteractions: R-25 requires keyboard navigation on the labelled chart image. */}
              <svg
                role="img"
                // biome-ignore lint/a11y/noNoninteractiveTabindex: R-25 chart is explicitly keyboard focusable.
                tabIndex={0}
                aria-label={`${metric === "cpu" ? "CPU" : "Memory"} by container, ${range}, ${series.map((s) => displayName(s.container)).join(", ") || "no containers"}`}
                viewBox="0 0 1000 240"
                preserveAspectRatio="none"
                className="h-60 w-full overflow-visible rounded-control focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                onPointerMove={(event) => {
                  if (pinned || !times.length) return;
                  const rect = event.currentTarget.getBoundingClientRect();
                  const time =
                    from +
                    ((event.clientX - rect.left) / rect.width) * (to - from);
                  setCursor(
                    times.reduce((a, b) =>
                      Math.abs(a - time) < Math.abs(b - time) ? a : b
                    )
                  );
                }}
                onPointerLeave={() => {
                  if (!pinned) setCursor(null);
                  onFocus(null);
                }}
                onClick={() => {
                  if (cursor !== null) setPinned((value) => !value);
                }}
                onKeyDown={(event) => {
                  if (["ArrowLeft", "ArrowRight"].includes(event.key)) {
                    event.preventDefault();
                    const index =
                      cursor === null
                        ? times.length - 1
                        : times.indexOf(cursor);
                    setCursor(
                      times[
                        Math.max(
                          0,
                          Math.min(
                            times.length - 1,
                            index + (event.key === "ArrowLeft" ? -1 : 1)
                          )
                        )
                      ] ?? null
                    );
                    setPinned(true);
                  }
                  if (["ArrowUp", "ArrowDown"].includes(event.key)) {
                    event.preventDefault();
                    const index = series.findIndex(
                      (s) => s.container.id === active
                    );
                    onFocus(
                      series[
                        (index +
                          (event.key === "ArrowUp" ? -1 : 1) +
                          series.length) %
                          series.length
                      ]?.container.id ?? null
                    );
                  }
                  if (event.key === "Enter") {
                    const row =
                      rows.find((s) => s.container.id === active) ?? rows[0];
                    if (row)
                      void navigate(link(row.container, row.point), {
                        state: containerReturnState(location),
                      });
                  }
                }}
              >
                {[0, 118, 236].map((y) => (
                  <line
                    key={y}
                    x1={0}
                    x2={1000}
                    y1={y}
                    y2={y}
                    stroke="var(--color-grid)"
                    vectorEffect="non-scaling-stroke"
                  />
                ))}
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
                            (p, index) => `${index ? "L" : "M"}${p.x},${p.y}`
                          )
                          .join(" ")}
                        fill="none"
                        stroke={seriesColor(s.slot)}
                        strokeWidth={active === s.container.id ? 3 : 2}
                        strokeLinecap="round"
                        strokeLinejoin="round"
                        vectorEffect="non-scaling-stroke"
                      />
                    ))}
                    {s.geometry.dots.map((dot) => (
                      <ellipse
                        key={dot.time}
                        cx={dot.x}
                        cy={dot.y}
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
                    y2={240}
                    stroke="var(--color-muted-foreground)"
                    vectorEffect="non-scaling-stroke"
                  />
                )}
              </svg>
              {brushStore && (
                <BrushOverlay
                  firstMs={from}
                  spanMs={to - from}
                  height={240}
                  inset={0}
                  keyboard={false}
                  onClickTime={() => {
                    if (cursor !== null) setPinned((value) => !value);
                  }}
                  onHoverTime={(time, fractionY) => {
                    if (pinned) return;
                    if (time === null || !times.length) {
                      setCursor(null);
                      onFocus(null);
                      return;
                    }
                    const nearest = times.reduce((a, b) =>
                      Math.abs(a - time) < Math.abs(b - time) ? a : b
                    );
                    setCursor(nearest);
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
                                  (1 - value / ceiling) * 236 - fractionY * 240
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
                  className={`top-2 ${pinned ? "pointer-events-auto" : ""} ${cursor > (from + to) / 2 ? "left-2" : "right-2"} max-w-full text-[11px]`}
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
            </div>
          </div>
        )}
        {!asTable && (
          <div className="figures mt-1 flex justify-between pl-12 text-[10px] text-muted-foreground">
            {[0, 1, 2, 3].map((tick) => (
              <span key={tick}>
                {tick === 3 && live
                  ? "now"
                  : clock(from + ((to - from) * tick) / 3)}
              </span>
            ))}
          </div>
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
      </div>
      <button
        type="button"
        className="w-fit rounded-control text-[11px] text-muted-foreground underline underline-offset-2 focus-visible:ring-2 focus-visible:ring-ring"
        onClick={() => setAsTable((value) => !value)}
      >
        {asTable ? "Show as chart" : "Show as table"}
      </button>
      {notice}
    </SectionCard>
  );
}
