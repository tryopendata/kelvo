/**
 * Window statistics over the live row ring, for module pages: averages, minima
 * and maxima over the last N seconds (residency and zone ranges over the
 * chart window, Kelvo's own CPU) and averages in buckets aligned to
 * wall-clock multiples (the per-core heatmap's columns over the chart window,
 * a downsampled hour of CPU total).
 *
 * They run over the grid live charts draw (`visitWindow`, D-090): a `mean` or
 * `rate` sample fills the slots it covers, so a plain mean of slots is
 * weighted by each sample's span, and a gauge is averaged over the straight
 * line between samples within a hold. Those fills are display values, never
 * written anywhere (data-boundary.md). A slot with no value adds nothing; a
 * bucket or window with no value is `null`, never 0 (error-handling.md
 * "Missing data is not an error").
 */
import {
  gridIntervalMs,
  visitWindow,
  type WindowSink,
  type WindowState,
  windowSlots,
} from "./live-state";

export type RingState = WindowState;

export interface SeriesStat {
  avg: number;
  min: number;
  max: number;
  /** Slots of the window with a value. */
  count: number;
}

/** Sum, count, min and max of the slots it is handed; reused per key. */
class StatSink implements WindowSink {
  sum = 0;
  n = 0;
  min = Infinity;
  max = -Infinity;
  reset(): this {
    this.sum = 0;
    this.n = 0;
    this.min = Infinity;
    this.max = -Infinity;
    return this;
  }
  put(_slot: number, v: number): void {
    this.sum += v;
    this.n += 1;
    if (v < this.min) this.min = v;
    if (v > this.max) this.max = v;
  }
}

const stat = new StatSink();

/** avg, min and max of each key over the last `windowMs`, on the grid. */
export function seriesStats(
  state: RingState,
  keys: readonly string[],
  windowMs: number
): Record<string, SeriesStat | null> {
  const out: Record<string, SeriesStat | null> = {};
  for (const key of keys) {
    visitWindow(state, key, windowMs, stat.reset());
    out[key] =
      stat.n === 0
        ? null
        : {
            avg: stat.sum / stat.n,
            min: stat.min,
            max: stat.max,
            count: stat.n,
          };
  }
  return out;
}

/**
 * Mean of one key over the last `windowMs` on the grid, `null` with no
 * value: Kelvo's own CPU in the popover footer and the settings overhead.
 */
export function windowMean(
  state: RingState,
  key: string,
  windowMs: number
): number | null {
  visitWindow(state, key, windowMs, stat.reset());
  return stat.n === 0 ? null : stat.sum / stat.n;
}

/** Adds each slot to the bucket its time falls in; reused per key. */
class BucketSink implements WindowSink {
  sums = new Float64Array(0);
  ns = new Uint32Array(0);
  /** Buckets in use; the arrays only grow. */
  count = 0;
  /** Time of slot 0, the grid's step, the bucket width and the first bucket. */
  t0 = 0;
  step = 1;
  bucketMs = 1;
  first = 0;
  put(slot: number, v: number): void {
    const b =
      Math.floor((this.t0 + slot * this.step) / this.bucketMs) - this.first;
    if (b < 0 || b >= this.count) return;
    this.sums[b] = (this.sums[b] as number) + v;
    this.ns[b] = (this.ns[b] as number) + 1;
  }
}

const buckets = new BucketSink();

/**
 * Average of each key per bucket, for bucket indexes `first..last` inclusive,
 * over the grid. Bucket `b` covers `[b * bucketMs, (b + 1) * bucketMs)`, so
 * columns stay put as time passes and only the newest one changes until it
 * closes.
 */
export function bucketAverages(
  state: RingState,
  keys: readonly string[],
  bucketMs: number,
  first: number,
  last: number
): Record<string, (number | null)[]> {
  const count = Math.max(0, last - first + 1);
  const out: Record<string, (number | null)[]> = {};
  const tEnd = state.lastTsMs;
  const step = gridIntervalMs(state.status);
  // The grid from the first bucket's start to the newest row.
  const windowMs = tEnd === null ? 0 : tEnd - first * bucketMs + step;
  const slots = windowSlots(windowMs, step);
  const b = buckets;
  if (b.ns.length < count) {
    b.sums = new Float64Array(count);
    b.ns = new Uint32Array(count);
  }
  b.count = count;
  b.t0 = (tEnd ?? 0) - (slots - 1) * step;
  b.step = step;
  b.bucketMs = bucketMs;
  b.first = first;
  for (const key of keys) {
    b.sums.fill(0, 0, count);
    b.ns.fill(0, 0, count);
    if (windowMs > 0) visitWindow(state, key, windowMs, b);
    const values = new Array<number | null>(count);
    for (let i = 0; i < count; i++) {
      const n = b.ns[i] as number;
      values[i] = n === 0 ? null : (b.sums[i] as number) / n;
    }
    out[key] = values;
  }
  return out;
}

/**
 * How many of the newest buckets can still change: the open one, plus every
 * closed one the next sample of `keys` can fill back into. A `mean` or `rate`
 * sample fills the slots back to the previous one, and a gauge's line runs
 * back to it, as long as they are within that sample's hold (D-090); so a
 * bucket is final only once it ended a full hold before the newest row. At
 * least two, so a bucket that just closed is always recomputed. Callers
 * memoize the buckets before these and recompute these each tick.
 */
export function liveBucketCount(
  state: RingState,
  keys: readonly string[],
  bucketMs: number
): number {
  const cols = state.columns;
  let hold = 0;
  if (cols.length > 0) {
    for (const k of keys)
      hold = Math.max(hold, cols.holdAt(cols.length - 1, k));
  }
  return Math.max(2, 1 + Math.ceil(hold / bucketMs));
}

/** Heatmap bucket steps. The interval caps at 60 s, so 60 s is always enough. */
const HEATMAP_BUCKETS_MS = [5000, 15_000, 30_000, 60_000];

/** Columns a heatmap aims for across its window. */
const HEATMAP_COLUMNS = 60;

/**
 * Bucket width for a per-series heatmap over `windowMs`: the first step that
 * keeps it to about 60 columns (5m → 5 s, 15m → 15 s, 30m → 30 s, 1h → 60 s),
 * raised to the first step at or above `intervalMs` so no column sits empty
 * between samples.
 */
export function heatmapBucketMs(windowMs: number, intervalMs: number): number {
  const floor = Math.max(windowMs / HEATMAP_COLUMNS, intervalMs);
  return (
    HEATMAP_BUCKETS_MS.find((b) => b >= floor) ??
    (HEATMAP_BUCKETS_MS.at(-1) as number)
  );
}

/** The bucket index holding `tsMs`. */
export function bucketIndex(tsMs: number, bucketMs: number): number {
  return Math.floor(tsMs / bucketMs);
}
