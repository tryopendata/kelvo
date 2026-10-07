/**
 * Time-range brush math for live charts (D-089): selections snap to the
 * 10 s buckets per-app network history is kept in, so a drawn selection is
 * always the range the totals cover and never implies 1 s precision.
 * Ranges are half-open, `[fromMs, toMs)`, in wall-clock milliseconds.
 */

import { clamp01 } from "@core/chart-math";
import { formatSpan } from "@core/format";
import { NET_BUCKET_MS } from "@core/generated/bindings";
import { windowWords } from "@core/live-window";
import { ceilTo as ceilToGrid, floorTo as floorToGrid } from "@core/time-grid";

/** Width of the buckets a selection snaps to: the per-app network bucket. */
export const BRUSH_STEP_MS: number = NET_BUCKET_MS;

export interface TimeRange {
  fromMs: number;
  toMs: number;
}

/** Bar widths a brushable chart may use: each divides 10 s or is a multiple of it. */
const BRUSH_BAR_LADDER_MS = [
  1000, 2000, 5000, 10_000, 30_000, 60_000, 300_000, 600_000,
] as const;

/** `time-grid` snapping on the brush's 10 s grid unless a step is given. */
export const floorTo = (t: number, step = BRUSH_STEP_MS) =>
  floorToGrid(t, step);

export const ceilTo = (t: number, step = BRUSH_STEP_MS) => ceilToGrid(t, step);

/**
 * Bar width for a brushable chart: the smallest width on the ladder that is
 * at least `minBucketMs` (what the chart would use without a brush) and the
 * sample interval. 1m → 1 s, 5m → 5 s, 15m → 10 s, 1h → 30 s at 1 s sampling.
 */
export function brushBucketMs(minBucketMs: number, intervalMs: number): number {
  const need = Math.max(minBucketMs, intervalMs);
  return BRUSH_BAR_LADDER_MS.find((w) => w >= need) ?? need;
}

/** The 10 s bucket holding `tMs`. */
export function bucketAt(tMs: number, step = BRUSH_STEP_MS): TimeRange {
  const fromMs = floorTo(tMs, step);
  return { fromMs, toMs: fromMs + step };
}

/**
 * The snapped range covering both ends of a drag, in either direction: the
 * earlier end down to a bucket edge, the later one up. Never empty.
 */
export function snapRange(
  aMs: number,
  bMs: number,
  step = BRUSH_STEP_MS
): TimeRange {
  const fromMs = floorTo(Math.min(aMs, bMs), step);
  const toMs = Math.max(fromMs + step, ceilTo(Math.max(aMs, bMs), step));
  return { fromMs, toMs };
}

/**
 * The range from the bucket starting at `anchorMs` to the one starting at
 * `focusMs`, both included (keyboard Shift+arrows).
 */
export function extendRange(
  anchorMs: number,
  focusMs: number,
  step = BRUSH_STEP_MS
): TimeRange {
  return {
    fromMs: floorTo(Math.min(anchorMs, focusMs), step),
    toMs: floorTo(Math.max(anchorMs, focusMs), step) + step,
  };
}

/** `r` cut to `[minMs, maxMs)`, keeping at least one bucket. */
export function clampRange(
  r: TimeRange,
  minMs: number,
  maxMs: number,
  step = BRUSH_STEP_MS
): TimeRange {
  const lo = floorTo(minMs, step);
  const hi = Math.max(lo + step, ceilTo(maxMs, step));
  const fromMs = Math.min(Math.max(r.fromMs, lo), hi - step);
  const toMs = Math.max(fromMs + step, Math.min(r.toMs, hi));
  return { fromMs, toMs };
}

/** Time under a pointer `fraction` (0..1) across a chart showing `[fromMs, fromMs + spanMs)`. */
export function timeAt(fraction: number, fromMs: number, spanMs: number) {
  return fromMs + clamp01(fraction) * spanMs;
}

/**
 * Where `r` sits on a chart showing `[fromMs, fromMs + spanMs)`, as left and
 * width fractions, cut to the chart; null when none of it is on the chart.
 */
export function rangeFractions(
  r: TimeRange,
  fromMs: number,
  spanMs: number
): { left: number; width: number } | null {
  const toMs = fromMs + spanMs;
  const a = Math.max(r.fromMs, fromMs);
  const b = Math.min(r.toMs, toMs);
  if (b <= a || spanMs <= 0) return null;
  return { left: (a - fromMs) / spanMs, width: (b - a) / spanMs };
}

/**
 * Bar slots `[from, to)` a range covers, for bars `bucketMs` wide where slot
 * 0 starts at `firstMs`; a bar counts when any of it is in the range.
 */
export function rangeSlots(
  r: TimeRange,
  firstMs: number,
  bucketMs: number,
  count: number
): { from: number; to: number } {
  const from = Math.floor((r.fromMs - firstMs) / bucketMs);
  const to = Math.ceil((r.toMs - firstMs) / bucketMs);
  return {
    from: Math.min(count, Math.max(0, from)),
    to: Math.min(count, Math.max(0, to)),
  };
}

export const sameRange = (a: TimeRange | null, b: TimeRange | null) =>
  a === b ||
  (a !== null && b !== null && a.fromMs === b.fromMs && a.toMs === b.toMs);

/** The whole chart window as 10 s buckets ending at `edgeMs`. */
export function windowRange(edgeMs: number, windowMs: number): TimeRange {
  return { fromMs: edgeMs - windowMs, toMs: edgeMs };
}

/** "last 15 minutes" or "selected 90 s", for a title. */
export function scopeWords(
  selection: TimeRange | null,
  windowMs: number
): string {
  return selection
    ? `selected ${formatSpan(selection.toMs - selection.fromMs)}`
    : `last ${windowWords(windowMs)}`;
}
