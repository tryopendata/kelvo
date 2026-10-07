export interface MinMaxAvg {
  min: number;
  max: number;
  avg: number;
}

/**
 * Reduce a series to `buckets` min/max/avg points, so a chart where one pixel
 * covers several samples can draw the avg line plus a min/max band and a 1 s
 * spike is not averaged away.
 *
 * Bucket `b` covers input indices `[floor(b*n/buckets), floor((b+1)*n/buckets))`,
 * so every sample lands in exactly one bucket. Nulls are skipped; a bucket
 * with no present samples is `null` (a gap), never zero. When the series is
 * already no longer than `buckets`, each sample is its own bucket.
 */
export function downsampleMinMaxAvg(
  series: readonly (number | null)[],
  buckets: number
): (MinMaxAvg | null)[] {
  const n = series.length;
  const count = Math.min(n, Math.max(0, Math.floor(buckets)));
  const out: (MinMaxAvg | null)[] = [];
  for (let b = 0; b < count; b++) {
    const from = Math.floor((b * n) / count);
    const to = Math.floor(((b + 1) * n) / count);
    let min = Number.POSITIVE_INFINITY;
    let max = Number.NEGATIVE_INFINITY;
    let sum = 0;
    let present = 0;
    for (let i = from; i < to; i++) {
      const v = series[i];
      if (v == null || !Number.isFinite(v)) continue;
      if (v < min) min = v;
      if (v > max) max = v;
      sum += v;
      present += 1;
    }
    out.push(present === 0 ? null : { min, max, avg: sum / present });
  }
  return out;
}
