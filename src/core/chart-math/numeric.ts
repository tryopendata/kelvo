/**
 * Fractions for bars, rings and shares. A missing input stays `null` (an
 * empty track, a "—"), never a 0 that reads as a measurement.
 */

/** Clamp a fraction to [0, 1]; NaN and ±Infinity become 0. */
export function clamp01(f: number): number {
  return Number.isFinite(f) ? Math.min(1, Math.max(0, f)) : 0;
}

/** `part / whole`, or null when either is missing or `whole` is not positive. */
export function ratio(
  part: number | null,
  whole: number | null
): number | null {
  if (part === null || whole === null || whole <= 0) return null;
  return part / whole;
}

/** `part` as a percent of `whole`, with `ratio`'s null rules. */
export function percentOf(
  part: number | null,
  whole: number | null
): number | null {
  const r = ratio(part, whole);
  return r === null ? null : r * 100;
}
