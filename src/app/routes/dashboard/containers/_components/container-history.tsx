import { formatBytes, formatPercent, formatRate } from "@core/format";
import type {
  DockerEvent,
  DockerHistoryPage,
  DockerMetrics,
} from "@core/generated/bindings";
import { memo, useMemo } from "react";
import { SectionCard } from "~/components/section-card";

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
  events?: readonly DockerEvent[];
  compact?: boolean;
  aggregate?: boolean;
}) {
  const memory = metric.includes("bytes");
  const format = memory
    ? formatBytes
    : metric === "cpu_pct"
      ? formatPercent
      : formatRate;
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
      const covered = p.measured_ms >= p.expected_ms && p.measured_ms > 0;
      if (value === null || !covered) {
        contiguous = false;
        limitContiguous = false;
        previousEnd = null;
        continue;
      }
      if (previousEnd !== null && p.time_ms > previousEnd) contiguous = false;
      path += `${contiguous ? "L" : "M"}${x.toFixed(2)},${(120 - (value / ceiling) * 115).toFixed(2)} `;
      contiguous = true;
      previousEnd = p.time_ms + p.duration_ms;
      const allowance = aggregate
        ? p.engine_capacity_bytes
        : p.explicit_limit_bytes;
      if (memory && allowance !== null) {
        const y = (120 - (allowance / ceiling) * 115).toFixed(2);
        limit += limitContiguous
          ? `H${x.toFixed(2)} V${y} `
          : `M${x.toFixed(2)},${y} `;
        limitContiguous = true;
      } else limitContiguous = false;
    }
    return { path, limit, ceiling };
  }, [page, metric, memory, from, to, aggregate]);
  return (
    <SectionCard
      accent={memory ? "mem" : "cpu"}
      title={title}
      aside={
        <span className="figures text-[11px] text-muted-foreground">
          {format(chart.ceiling)}
        </span>
      }
    >
      {page?.points.length ? (
        <>
          <svg
            role="img"
            aria-label={`${title}, gaps are unmeasured intervals`}
            viewBox="0 0 1000 124"
            className={
              compact
                ? "h-20 w-full overflow-visible"
                : "h-32 w-full overflow-visible"
            }
            preserveAspectRatio="none"
          >
            <line
              x1="0"
              x2="1000"
              y1="120"
              y2="120"
              stroke="var(--color-axis)"
              vectorEffect="non-scaling-stroke"
            />
            <path
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
                y1="108"
                y2="120"
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
          <div className="figures flex justify-between text-[10px] text-muted-foreground">
            <span>{new Date(from).toLocaleString()}</span>
            <span>{new Date(to).toLocaleTimeString()}</span>
          </div>
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
      {page?.coverage.map((gap) => (
        <p
          key={`${gap.start_ms}-${gap.container_id}`}
          className="text-[11px] text-muted-foreground"
        >
          Coverage gap: {gap.reason}
        </p>
      ))}
    </SectionCard>
  );
});
