/**
 * Autoscaled chart ceilings (network, power, the popover CPU chart): snap the
 * data max up to a nice step, grow at once, and shrink only after the data has
 * stayed at or below the next step down for 60 s, so the scale does not jitter
 * every tick (design-system.md, "Chart rules").
 */

/** Mantissas of the nice ladder, times powers of ten: …, 8, 10, 20, 40, 50, … */
const LADDER = [1, 2, 4, 5, 6, 8, 10] as const;

export const SHRINK_AFTER_MS = 60_000;

/**
 * Smallest ladder step `>= value`. Values at or below zero (and missing ones)
 * get `minCeiling`, so an idle chart still has a labelled scale.
 */
export function niceCeiling(value: number, minCeiling = 1): number {
  if (!Number.isFinite(value) || value <= minCeiling) return minCeiling;
  const exp = Math.floor(Math.log10(value));
  const base = 10 ** exp;
  for (const m of LADDER) {
    // Round away float noise (0.3 / 0.1 = 2.9999999999999996).
    const step = Number((m * base).toPrecision(12));
    if (step >= value) return step;
  }
  return 10 * base;
}

/** The ladder step just below `ceiling`, or 0 when `ceiling` is the floor. */
export function stepBelow(ceiling: number, minCeiling = 1): number {
  if (ceiling <= minCeiling) return 0;
  const exp = Math.floor(Math.log10(ceiling));
  const base = 10 ** exp;
  const m = Number((ceiling / base).toPrecision(12));
  const i = LADDER.indexOf(m as (typeof LADDER)[number]);
  // `ceiling` is a ladder value with mantissa 1..8 here; mantissa 1 steps down
  // to 8 at the previous decade.
  const below = i > 0 ? (LADDER[i - 1] as number) * base : 0.8 * base;
  return Math.max(Number(below.toPrecision(12)), minCeiling);
}

export interface CeilingState {
  ceiling: number;
  /** When the data first dropped to or below the next step down, or null. */
  belowSinceMs: number | null;
  /** Highest value seen since `belowSinceMs`: what the shrink snaps to. */
  belowPeak: number;
  /** Time of the previous update, to spot sleeps and clock jumps. */
  lastMs: number;
}

export interface CeilingOptions {
  minCeiling?: number;
  shrinkAfterMs?: number;
  /**
   * A gap between updates longer than this (sleep, a stopped channel) or a
   * clock that moved backwards restarts the shrink timer: time nobody saw does
   * not count as time below. Default 10 s, above the slowest 5 s interval.
   */
  maxTickGapMs?: number;
}

/**
 * Advance the ceiling by one observation. `max` is the highest value currently
 * in the chart's window, or null when the window holds no samples (which holds
 * the ceiling and restarts the shrink timer). Pure: returns a new state.
 */
export function nextCeiling(
  prev: CeilingState | null,
  max: number | null,
  nowMs: number,
  {
    minCeiling = 1,
    shrinkAfterMs = SHRINK_AFTER_MS,
    maxTickGapMs = 10_000,
  }: CeilingOptions = {}
): CeilingState {
  const present = max != null && Number.isFinite(max);

  if (prev === null) {
    return {
      ceiling: niceCeiling(present ? max : 0, minCeiling),
      belowSinceMs: null,
      belowPeak: 0,
      lastMs: nowMs,
    };
  }

  if (!present) {
    return { ...prev, belowSinceMs: null, belowPeak: 0, lastMs: nowMs };
  }

  if (max > prev.ceiling) {
    return {
      ceiling: niceCeiling(max, minCeiling),
      belowSinceMs: null,
      belowPeak: 0,
      lastMs: nowMs,
    };
  }

  if (max > stepBelow(prev.ceiling, minCeiling)) {
    return { ...prev, belowSinceMs: null, belowPeak: 0, lastMs: nowMs };
  }

  const dt = nowMs - prev.lastMs;
  const continuous = dt >= 0 && dt <= maxTickGapMs;
  const continuing = prev.belowSinceMs !== null && continuous;
  const belowSinceMs = continuing ? (prev.belowSinceMs as number) : nowMs;
  const belowPeak = continuing ? Math.max(prev.belowPeak, max) : max;

  if (nowMs - belowSinceMs >= shrinkAfterMs) {
    return {
      ceiling: niceCeiling(belowPeak, minCeiling),
      belowSinceMs: null,
      belowPeak: 0,
      lastMs: nowMs,
    };
  }
  return { ceiling: prev.ceiling, belowSinceMs, belowPeak, lastMs: nowMs };
}
