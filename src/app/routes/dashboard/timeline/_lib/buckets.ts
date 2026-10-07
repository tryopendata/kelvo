/**
 * Timeline data: history buckets from `query_history` (plan 4.6, "Data by
 * range") and the aligned columns uPlot draws. Rust answers through now, its
 * uncommitted and open buckets included (D-092), so nothing is stitched from
 * the live ring.
 *
 * Missing data stays missing: a bucket with no measured value is absent.
 * Each series says how far apart two points can be and still be one line
 * (`hold_ms`, D-092); further apart, and wherever a gap starts, the columns
 * put a `null` between them, so no line or area is drawn across a hole
 * (error-handling.md).
 */
import type { HistoryPoint } from "@core/generated/bindings";

export type Bucket = HistoryPoint;

/** How the label sets of one metric merge: network sums, fans take the max. */
export type Combine = "sum" | "max";

const addOrNull = (a: number | null, b: number | null) =>
  a === null || b === null ? null : a + b;
const maxOrNull = (a: number | null, b: number | null) =>
  a === null ? b : b === null ? a : Math.max(a, b);

/**
 * Merge several series of one metric (one per interface, per fan) into one
 * series by bucket time. For a sum the envelope is the sum of the parts'
 * minima and maxima, which bounds the total's own range from outside: it can
 * only look wider than it was, never hide a spike.
 */
export function combineSeries(
  series: readonly { points: readonly Bucket[] }[],
  combine: Combine
): Bucket[] {
  if (series.length === 1) {
    return (series[0]?.points ?? []).filter((p) => p.avg !== null);
  }
  const byT = new Map<number, Bucket>();
  for (const s of series) {
    for (const p of s.points) {
      if (p.avg === null) continue;
      const cur = byT.get(p.t);
      if (!cur) {
        byT.set(p.t, { ...p });
      } else if (combine === "sum") {
        cur.avg = (cur.avg ?? 0) + p.avg;
        cur.min = addOrNull(cur.min, p.min);
        cur.max = addOrNull(cur.max, p.max);
      } else {
        cur.avg = Math.max(cur.avg ?? -Infinity, p.avg);
        cur.min = maxOrNull(cur.min, p.min);
        cur.max = maxOrNull(cur.max, p.max);
      }
    }
  }
  return [...byT.values()].sort((a, b) => a.t - b.t);
}

export interface GapSpan {
  fromMs: number;
  toMs: number;
}

/**
 * One series' value per slot of a shared x column: a number where it has a
 * bucket, `undefined` where it has none but its neighbours are within its
 * hold (uPlot joins across it), `null` where its line breaks.
 */
export type Column = (number | null | undefined)[];

/** min, max and avg columns for one series, aligned to a shared x column. */
export interface SeriesColumns {
  min: Column;
  max: Column;
  avg: Column;
}

export interface AlignedColumns {
  x: number[];
  series: SeriesColumns[];
}

/**
 * Put several series on one x column for uPlot. Consecutive buckets of a
 * series no more than its `holds` entry apart are one line: slots between
 * them that are other series' are `undefined` for it. Further apart, its
 * slots between them are `null`, and where there is no slot between them
 * one is inserted a bucket after the first. A gap's start is a slot that is
 * `null` for every series; buckets inside a gap are dropped.
 */
export function alignColumns(
  seriesList: readonly (readonly Bucket[])[],
  holds: readonly number[],
  bucketMs: number,
  gaps: readonly GapSpan[] = []
): AlignedColumns {
  const inGap = (t: number) => gaps.some((g) => t >= g.fromMs && t < g.toMs);
  const kept = seriesList.map((s) => s.filter((b) => !inGap(b.t)));
  const times = new Set<number>();
  for (const s of kept) for (const b of s) times.add(b.t);
  const breakAt = new Set(gaps.map((g) => g.fromMs));
  for (const t of breakAt) times.add(t);
  // A hole a bucket after the last point before a break, where the series
  // has no other slot to put its `null` in.
  kept.forEach((s, k) => {
    const hold = holds[k] ?? bucketMs;
    for (let i = 1; i < s.length; i++) {
      const t0 = (s[i - 1] as Bucket).t;
      const t1 = (s[i] as Bucket).t;
      if (t1 - t0 > hold) times.add(Math.min(t0 + bucketMs, t1 - 1));
    }
  });
  const x = [...times].sort((a, b) => a - b);

  const series = kept.map((s, k) => {
    const hold = holds[k] ?? bucketMs;
    const avg: Column = [];
    const min: Column = [];
    const max: Column = [];
    let j = 0;
    for (const t of x) {
      while (j < s.length && (s[j] as Bucket).t < t) j++;
      const b = s[j];
      if (b && b.t === t) {
        avg.push(b.avg);
        min.push(b.min);
        max.push(b.max);
        continue;
      }
      const prev = s[j - 1];
      const joined =
        !breakAt.has(t) && prev && b && b.t - prev.t <= hold ? undefined : null;
      avg.push(joined);
      min.push(joined);
      max.push(joined);
    }
    return { min, max, avg };
  });
  return { x, series };
}

/**
 * The bucket containing `tMs`: the last bucket starting at or before it,
 * if `tMs` falls inside its width. Binary search; buckets are sorted.
 */
export function bucketAt(
  buckets: readonly Bucket[],
  tMs: number,
  bucketMs: number
): Bucket | null {
  let lo = 0;
  let hi = buckets.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if ((buckets[mid] as Bucket).t <= tMs) lo = mid + 1;
    else hi = mid;
  }
  const b = buckets[lo - 1];
  return b && tMs < b.t + bucketMs ? b : null;
}

/**
 * Indices of the measured slots that touch a gap band: the hollow dots. A
 * slot touches when the series' line would have reached the band: its
 * bucket ends no more than `holdMs` (the series' `hold_ms`, at least a
 * bucket) before the band starts, or starts no more than that after it
 * ends. A series sampled every minute still gets its dot on a 10 s page.
 */
export function gapEdgeIndices(
  x: readonly number[],
  avg: Readonly<Column>,
  gaps: readonly GapSpan[],
  bucketMs: number,
  holdMs: number
): number[] {
  const out = new Set<number>();
  for (const g of gaps) {
    for (let i = x.length - 1; i >= 0; i--) {
      const t = x[i] as number;
      if (t >= g.fromMs || avg[i] == null) continue;
      if (t + bucketMs + holdMs >= g.fromMs) out.add(i);
      break;
    }
    for (let i = 0; i < x.length; i++) {
      const t = x[i] as number;
      if (t < g.toMs || avg[i] == null) continue;
      if (t - holdMs <= g.toMs) out.add(i);
      break;
    }
  }
  return [...out].sort((a, b) => a - b);
}

/** Highest max in the series and when it was, for "peak 71% · 14:02". */
export function peakOf(
  buckets: readonly Bucket[]
): { value: number; tMs: number } | null {
  let best: { value: number; tMs: number } | null = null;
  for (const b of buckets) {
    const v = b.max ?? b.avg;
    if (v !== null && (best === null || v > best.value)) {
      best = { value: v, tMs: b.t };
    }
  }
  return best;
}

/** Mean of the bucket averages, for "avg 27% · 1.1 GHz". */
export function meanOf(buckets: readonly Bucket[]): number | null {
  let sum = 0;
  let n = 0;
  for (const b of buckets) {
    if (b.avg === null) continue;
    sum += b.avg;
    n += 1;
  }
  return n > 0 ? sum / n : null;
}

/** Highest value in the series (max, else avg), or null when empty. */
export function maxOf(buckets: readonly Bucket[]): number | null {
  return peakOf(buckets)?.value ?? null;
}
