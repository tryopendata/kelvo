import { formatBytes, formatPercent, formatRate } from "@core/format";
import type {
  DockerEvent,
  DockerHistoryPage,
  DockerMetrics,
} from "@core/generated/bindings";
import { memo, useMemo } from "react";
import { SectionCard } from "~/components/section-card";
import { ChartFrame, ChartGrid, chartY } from "~/widgets/chart-frame";
import { historyTicks } from "~/widgets/lib/chart-labels";

/** Draw true timestamp positions; uncovered buckets break the path. */
export const ContainerHistory = memo(function ContainerHistory({
  page,
  metric,
  title,
  from,
  to,
  events = [],
  compact = false,
  aggregate = false,
}: {
  page: DockerHistoryPage | undefined;
  metric: keyof DockerMetrics;
  title: string;
  from: number;
  to: number;
  events?: readonly DockerEvent[] | undefined;
  compact?: boolean;
  aggregate?: boolean;
}) {
  const height = compact ? 80 : 128;
  const memory = metric.includes("bytes");
  const format = memory
    ? (value: number) =>
        formatBytes(value, value > 0 && value < 1 ? { decimals: 1 } : {})
    : metric === "cpu_pct"
      ? (value: number) => formatPercent(value, { decimals: 1 })
      : (value: number) =>
          formatRate(value, value > 0 && value < 1 ? { decimals: 1 } : {});
  // biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Controller and presentation split tracked in architecture refactor package H.
  const chart = useMemo(() => {
    const points = page?.points ?? [];
    const ceiling = Math.max(
      1,
      ...points.map((p) => p.metrics[metric] ?? 0),
      ...(memory
        ? points.map(
            (p) =>
              (aggregate ? p.engine_capacity_bytes : p.explicit_limit_bytes) ??
              0
          )
        : [])
    );
    let path = "";
    let limit = "";
    let contiguous = false;
    let limitContiguous = false;
    let previousEnd: number | null = null;
    for (const p of points) {
      const value = p.metrics[metric];
      const x = ((p.time_ms - from) / Math.max(1, to - from)) * 1000;
      if (value === null || !(memory ? p.memory_covered : p.cpu_covered)) {
        contiguous = false;
        limitContiguous = false;
        previousEnd = null;
        continue;
      }
      if (previousEnd !== null && p.time_ms > previousEnd) contiguous = false;
      path += `${contiguous ? "L" : "M"}${x.toFixed(2)},${chartY(value, ceiling, height).toFixed(2)} `;
      contiguous = true;
      previousEnd = p.time_ms + p.duration_ms;
      const allowance = aggregate
        ? p.engine_capacity_bytes
        : p.explicit_limit_bytes;
      if (memory && allowance !== null) {
        const y = chartY(allowance, ceiling, height).toFixed(2);
        limit += limitContiguous
          ? `H${x.toFixed(2)} V${y} `
          : `M${x.toFixed(2)},${y} `;
        limitContiguous = true;
      } else limitContiguous = false;
    }
    return { path, limit, ceiling };
  }, [page, metric, memory, from, to, aggregate, height]);
  return (
    <SectionCard accent={memory ? "mem" : "cpu"} title={title}>
      {page?.points.length ? (
        <>
          <ChartFrame
            height={height}
            ceiling={chart.ceiling}
            gutter={48}
            yTicks={[chart.ceiling, chart.ceiling / 2].map((value) => ({
              value,
              label: format(value),
            }))}
            xTicks={historyTicks(from, to, false, 3)}
          >
            <svg
              role="img"
              aria-label={`${title}, gaps are unmeasured intervals`}
              viewBox={`0 0 1000 ${height}`}
              height={height}
              className="block w-full overflow-hidden"
              preserveAspectRatio="none"
            >
              <ChartGrid
                values={[chart.ceiling, chart.ceiling / 2]}
                ceiling={chart.ceiling}
                height={height}
              />
              <path
                data-line
                d={chart.path}
                fill="none"
                stroke={
                  memory
                    ? "var(--color-mem-ink, var(--color-mem))"
                    : "var(--color-cpu-ink, var(--color-cpu))"
                }
                strokeWidth="1.5"
                vectorEffect="non-scaling-stroke"
              />
              {chart.limit && (
                <path
                  d={chart.limit}
                  fill="none"
                  stroke="var(--color-muted-foreground)"
                  strokeDasharray="4 4"
                  vectorEffect="non-scaling-stroke"
                />
              )}
              {events.map((event) => (
                <line
                  key={event.id}
                  x1={((event.time_ms - from) / (to - from)) * 1000}
                  x2={((event.time_ms - from) / (to - from)) * 1000}
                  y1={height - 12}
                  y2={height - 1}
                  stroke="var(--color-muted-foreground)"
                  vectorEffect="non-scaling-stroke"
                >
                  <title>
                    {event.action} ·{" "}
                    {new Date(event.time_ms).toLocaleTimeString()}
                  </title>
                </line>
              ))}
            </svg>
          </ChartFrame>
          {memory && (
            <p className="text-[11px] text-muted-foreground">
              Working set · dashed line:{" "}
              {aggregate ? "engine capacity" : "explicit memory limit"} at that
              time
            </p>
          )}
        </>
      ) : (
        <p className="py-6 text-[12px] text-muted-foreground">
          {page
            ? "No recorded measurements in this range. Monitoring may have been off or history expired."
            : "Loading recorded measurements…"}
        </p>
      )}
    </SectionCard>
  );
});
