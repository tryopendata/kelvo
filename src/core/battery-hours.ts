/**
 * The "Battery, last 24 hours" bars (plan 4.10). Rust's
 * `battery_hours` decides each hour's bar: the charge in its last minute,
 * charging when `battery.charging` was on in any minute, `null` (hatched)
 * when no minute sampled it, never 0. Rust knows no time zone, so the client
 * sends the local hour boundaries, as for the heatmap (heatmap-days.ts).
 */
import type { BatteryHour as HourRow } from "@core/generated/bindings";

export interface BatteryHour {
  /** Start of the local hour, ms epoch. */
  tsMs: number;
  charge: number | null;
  charging: boolean;
}

const HOUR_MS = 3_600_000;

/**
 * The `hours + 1` boundaries (UTC ms) of the `hours` real hours ending with
 * the local hour that holds `now`, oldest first: the last is the end of the
 * current hour, the right edge of the bars. Stepped back in real hours from
 * the current local hour's start, so a DST change neither leaves a bar empty
 * (spring forward) nor folds two hours into one (fall back): every bar is an
 * hour long. In a half-hour zone the hours start at :30 UTC.
 */
export function batteryHourStarts(now: Date, hours = 24): number[] {
  // The local hour's start from the local minutes, not `new Date(y, m, d,
  // h)`, which picks the first of a fall-back's two 1 a.m.s.
  const start =
    now.getTime() -
    now.getMinutes() * 60_000 -
    now.getSeconds() * 1000 -
    now.getMilliseconds();
  const starts: number[] = [];
  for (let i = hours - 1; i >= -1; i--) starts.push(start - i * HOUR_MS);
  return starts;
}

/** Rust's rows in the bars' shape. */
export function batteryBars(rows: readonly HourRow[]): BatteryHour[] {
  return rows.map((r) => ({
    tsMs: r.start_ms,
    charge: r.charge,
    charging: r.charging,
  }));
}
