import type { HistoryPage } from "@core/generated/bindings";
import { METRIC_MODULES } from "@core/generated/bindings";
import { seriesKeyString } from "@core/series-key";

/** Lay server-averaged buckets on the chart grid without filling missing samples or gaps. */
export function historyGrid(
  page: HistoryPage,
  keys: readonly string[],
  fromMs: number,
  toMs: number,
  breakAtGaps = true
) {
  const bucketMs = page.bucket_ms;
  const anchor =
    page.series
      .flatMap((s) => s.points.slice(0, 1))
      .reduce<number | null>(
        (first, point) => (first === null ? point.t : Math.min(first, point.t)),
        null
      ) ?? fromMs;
  // Merged buckets are anchored by Rust to the requested source bucket, not Unix zero.
  const first = anchor - Math.ceil((anchor - fromMs) / bucketMs) * bucketMs;
  const count = Math.max(1, Math.ceil((toMs - first) / bucketMs));
  const values: Record<string, (number | null)[]> = {};
  for (const key of keys)
    values[key] = new Array<number | null>(count).fill(null);
  for (const series of page.series) {
    const key = seriesKeyString(series.key);
    const target = values[key];
    if (!target) continue;
    const module =
      METRIC_MODULES[series.key.metric as keyof typeof METRIC_MODULES];
    const gaps = page.gaps.filter(
      (gap) => gap.module === null || gap.module === module
    );
    for (const point of series.points) {
      const index = Math.floor((point.t - first) / bucketMs);
      if (index < 0 || index >= count) continue;
      const gap = gaps.some(
        (g) =>
          g.start_ms < point.t + bucketMs && (g.end_ms ?? Infinity) > point.t
      );
      if (!breakAtGaps || !gap) target[index] = point.avg;
    }
  }
  return { values, endMs: first + (count - 1) * bucketMs, bucketMs };
}
