/**
 * How big history gets for a retention, and how many days fit under the
 * "History size limit" (D-057, D-059, D-076). Settings shows it next to each
 * retention option; onboarding step 2 quotes it.
 *
 * History is a fixed part that does not depend on retention (24 h of 10 s
 * buckets, 72 h of process snapshots), the last 7 days at 1-minute
 * resolution, and anything older at 15 minutes (D-076). A day of quarters
 * is 96 rows against a minute day's 1,440 of the same width, so it is
 * costed at a fifteenth of a minute day.
 *
 * The model comes from two pairs of fill tests with 16 KiB pages, size
 * after close, at 150 and 250 series:
 *
 * - before roll-down (D-057), 30 days all in minutes: 139.7 and 203.4 MB;
 * - with roll-down (D-076), 30 days as 7 of minutes and 23 of quarters:
 *   69.9 and 95.7 MB.
 *
 * Minute rows did not change between them, so each pair gives the fixed
 * part and the cost of a minute day at that series count. Both are taken as
 * lines through 150 and 250 series. The model also lands on D-057's capped
 * run (250 series, 17.4 days of minutes, 139.9 MB) within 0.1 MB. It is an
 * estimate: other series counts are extrapolated.
 *
 * The model's inputs (the fill test runs, the minute window, the tier
 * widths and the trim's low water) are Rust's, generated as
 * `HISTORY_PROJECTION` (D-092).
 *
 * Once a machine has recorded an hour, Rust measures both numbers from its
 * own file (`history_growth`), and those replace the fill tests': real
 * processes and readings cost less than the tests' incompressible ones (a
 * 105-series Mac measured 36 MB fixed and 2.1 MB per minute day, where the
 * fill tests give 37 and 2.5).
 */

import {
  HISTORY_PROJECTION,
  type HistoryGrowth,
} from "@core/generated/bindings";

const MB = 1_000_000;

/** Days kept at 1-minute resolution before the roll-down (D-076). */
const MINUTE_DAYS = HISTORY_PROJECTION.minute_days;
/** Minute rows per 15-minute row. */
const QUARTER_FACTOR = HISTORY_PROJECTION.quarter_factor;

interface FillRun {
  series: number;
  days: number;
  bytes: number;
}

const run = (r: { series: number; days: number; mb: number }): FillRun => ({
  series: r.series,
  days: r.days,
  bytes: r.mb * MB,
});

/** Fill test results (bytes after close), smaller series count first. */
export const FILL_TEST = {
  /** D-057: every day in minutes. */
  minutes: {
    small: run(HISTORY_PROJECTION.fill_minutes[0]),
    large: run(HISTORY_PROJECTION.fill_minutes[1]),
  },
  /** D-076: 10,080 minute rows and 2,208 quarter rows per series. */
  rolled: {
    small: run(HISTORY_PROJECTION.fill_rolled[0]),
    large: run(HISTORY_PROJECTION.fill_rolled[1]),
  },
} as const;

/** A retention in minute days: 7 at full cost, the rest at a fifteenth. */
function minuteDayEquivalents(days: number): number {
  const minutes = Math.min(days, MINUTE_DAYS);
  return minutes + (days - minutes) / QUARTER_FACTOR;
}

/** Fixed part and minute-day cost at one fill test series count. */
function solve(
  minutes: { days: number; bytes: number },
  rolled: typeof minutes
) {
  const perDay =
    (minutes.bytes - rolled.bytes) /
    (minutes.days - minuteDayEquivalents(rolled.days));
  return { fixed: minutes.bytes - minutes.days * perDay, perDay };
}

const SMALL = solve(FILL_TEST.minutes.small, FILL_TEST.rolled.small);
const LARGE = solve(FILL_TEST.minutes.large, FILL_TEST.rolled.large);
const SERIES_STEP =
  FILL_TEST.minutes.large.series - FILL_TEST.minutes.small.series;

/** The line through the small and large fill tests, at `series`. */
function line(small: number, large: number, series: number): number {
  return (
    small +
    ((large - small) * (series - FILL_TEST.minutes.small.series)) / SERIES_STEP
  );
}

/** Series count to assume before the live layout arrives (the fill test's). */
export const DEFAULT_SERIES: number = FILL_TEST.minutes.small.series;

/**
 * A trim stops at its low water (90% of the limit) and the file grows back
 * to the limit before the next trim, so history spends its time between the
 * two. Days are quoted at the middle.
 */
const LIMIT_FILL = (1 + HISTORY_PROJECTION.trim_low_water) / 2;

/** Bytes that do not depend on retention, for `series` series. */
export function fixedBytes(series: number): number {
  return line(SMALL.fixed, LARGE.fixed, series);
}

/** Bytes one day of 1-minute history costs, for `series` series. */
export function bytesPerMinuteDay(series: number): number {
  return line(SMALL.perDay, LARGE.perDay, series);
}

/** The fixed part and minute-day cost: measured when given, else the fill tests'. */
function model(
  series: number,
  growth: HistoryGrowth | null | undefined
): { fixed: number; perDay: number } {
  return growth
    ? { fixed: growth.fixed_bytes, perDay: growth.minute_day_bytes }
    : { fixed: fixedBytes(series), perDay: bytesPerMinuteDay(series) };
}

/** Projected size of `days` of history with no size limit. */
export function projectedHistoryBytes(
  days: number,
  series = DEFAULT_SERIES,
  growth?: HistoryGrowth | null
): number {
  const m = model(series, growth);
  return m.fixed + minuteDayEquivalents(days) * m.perDay;
}

/**
 * Whole days of history that fit under `limitBytes`. The trim drops the
 * oldest data first, so the quarters go before the minutes (D-076). Never
 * under 1: the trim keeps the last 24 hours even when they do not fit
 * (D-057).
 */
export function daysUnderLimit(
  limitBytes: number,
  series = DEFAULT_SERIES,
  growth?: HistoryGrowth | null
): number {
  const { fixed, perDay } = model(series, growth);
  const room = limitBytes * LIMIT_FILL - fixed;
  const minuteRoom = MINUTE_DAYS * perDay;
  if (room < minuteRoom) return Math.max(1, Math.floor(room / perDay));
  return (
    MINUTE_DAYS + Math.floor(((room - minuteRoom) * QUARTER_FACTOR) / perDay)
  );
}

export interface RetentionProjection {
  /** Expected size on disk: the projection, or the limit when it is lower. */
  bytes: number;
  /** Days actually kept when the limit cuts the retention short, else null. */
  limitedDays: number | null;
}

/** What a retention option will cost and keep under a size limit. */
export function retentionProjection(
  days: number,
  limitBytes: number,
  series = DEFAULT_SERIES,
  growth?: HistoryGrowth | null
): RetentionProjection {
  const bytes = projectedHistoryBytes(days, series, growth);
  if (bytes <= limitBytes) return { bytes, limitedDays: null };
  return {
    bytes: limitBytes,
    limitedDays: Math.min(days, daysUnderLimit(limitBytes, series, growth)),
  };
}

/** "140 MB", "1 GB": sizes rounded to 10 MB for an estimate. */
export function approxSize(bytes: number): string {
  if (bytes >= 1000 * MB) {
    const gb = Math.round(bytes / (100 * MB)) / 10;
    return `${gb} GB`;
  }
  return `${Math.max(10, Math.round(bytes / (10 * MB)) * 10)} MB`;
}

/** "150 MB", "1 GB" for a size limit option. */
export function sizeLimitLabel(mb: number): string {
  return mb >= 1000 ? `${mb / 1000} GB` : `${mb} MB`;
}
