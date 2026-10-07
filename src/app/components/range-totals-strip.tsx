import { formatSpan, MISSING } from "@core/format";
import type { MetricStat } from "@core/generated/bindings";
import { useMemo } from "react";
import { heldFor, useRangeScope } from "~/hooks/use-range-scope";
import { useSeriesStats } from "~/hooks/use-series-stats";
import { StatStrip } from "~/widgets/stat-strip";

export interface RangeTotal {
  /** An unlabelled catalog metric (`cpu.total`). */
  metric: string;
  /** "Avg", "Peak", "↓ Read": the strip appends the span. */
  label: string;
  /** The figure; `stat` is undefined until the answer arrives. */
  format: (stat: MetricStat) => string;
}

/**
 * A metric's measured time when it fell short of the range (asleep, the
 * module off), for "in 11 min" after its figure; null when it covered the
 * range. A second or 1% of slack absorbs sampling jitter.
 */
export function measuredShort(stat: MetricStat, spanMs: number): string | null {
  if (
    spanMs <= 0 ||
    stat.measured_ms >= spanMs - Math.max(1_000, spanMs * 0.01)
  ) {
    return null;
  }
  return formatSpan(stat.measured_ms);
}

/**
 * Figures over the chart window (D-091), or over the brushed range when
 * there is one (D-099), beside a page's live stat strip. The window ends
 * where the open 10 s bucket starts, so the figures change every 10 s.
 */
export function RangeTotalsStrip({
  windowMs,
  totals,
  testId,
}: {
  windowMs: number;
  totals: readonly RangeTotal[];
  testId?: string;
}) {
  const { selection, range, keepPrevious } = useRangeScope(windowMs);
  const metrics = useMemo(
    () => [...new Set(totals.map((t) => t.metric))],
    [totals]
  );
  const q = useSeriesStats(metrics, range, { keepPrevious });
  const shown = heldFor(q, windowMs);
  const scope = formatSpan(
    selection ? selection.toMs - selection.fromMs : windowMs
  );
  return (
    <div data-testid={testId}>
      <StatStrip
        items={totals.map((t) => {
          const stat = shown?.metrics.find((m) => m.metric === t.metric);
          const short =
            stat && shown
              ? measuredShort(stat, shown.to_ms - shown.from_ms)
              : null;
          return {
            label: `${t.label} · ${scope}`,
            value: stat ? t.format(stat) : MISSING,
            unit: short === null ? undefined : `in ${short}`,
          };
        })}
      />
    </div>
  );
}
