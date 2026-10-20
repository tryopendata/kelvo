/** Fixed-width buckets from `fromMs`, one value each; `null` is a gap. */
export interface BucketSeries {
  fromMs: number;
  bucketMs: number;
  values: readonly (number | null)[];
}

/** A chart's slot grid: `length` slots `intervalMs` apart, the last at `tEndMs`. */
export interface SlotGrid {
  tEndMs: number;
  intervalMs: number;
  length: number;
}

/**
 * Lays coarser buckets onto a chart's slot grid (10 s per-app buckets under a
 * 1 s or 5 s line): each slot takes the value of the bucket its timestamp
 * falls in. A slot before the first bucket or after the last is `null`, and
 * a gap bucket stays a gap; nothing is interpolated between buckets.
 */
export function resampleBuckets(
  buckets: BucketSeries,
  grid: SlotGrid
): (number | null)[] {
  const out: (number | null)[] = new Array(grid.length);
  const { fromMs, bucketMs, values } = buckets;
  for (let i = 0; i < grid.length; i++) {
    const t = grid.tEndMs - (grid.length - 1 - i) * grid.intervalMs;
    const k = bucketMs > 0 ? Math.floor((t - fromMs) / bucketMs) : -1;
    out[i] = k >= 0 && k < values.length ? (values[k] ?? null) : null;
  }
  return out;
}
