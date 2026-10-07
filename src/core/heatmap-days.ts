/**
 * The heatmap's local days (`query_heatmap`). Rust knows no time zones, so
 * the frontend turns "the last N local days" into each day's local-hour
 * boundaries in UTC ms, with DST applied once here by the JS `Date`:
 *
 * - a spring-forward hour that does not exist (02:00 in New York) starts
 *   where the next hour does, so its cell is empty and comes back `null`;
 * - a fall-back hour that happens twice (01:00) starts at its first
 *   occurrence and runs to 02:00 standard time, a two-hour cell.
 *
 * `new Date(y, m, d, h)` already does both: a nonexistent local time moves
 * forward past the jump, an ambiguous one resolves to the earlier instant.
 */
import { pad2 } from "@core/format";
import type { HeatmapDaySpec } from "@core/generated/bindings";

/** `2026-10-04` for the local date of `d`. */
export function localDateIso(d: Date): string {
  return `${d.getFullYear()}-${pad2(d.getMonth() + 1)}-${pad2(d.getDate())}`;
}

/** One local day's 25 boundaries: hours 00 to 23, then the next midnight. */
export function localHourStarts(
  year: number,
  month: number,
  day: number
): number[] {
  const starts: number[] = [];
  for (let h = 0; h < 24; h++) {
    starts.push(new Date(year, month, day, h).getTime());
  }
  starts.push(new Date(year, month, day + 1).getTime());
  // A nonexistent hour normally lands exactly on the next one's start. Clamp
  // anyway: Rust rejects boundaries that go backwards, and an odd zone rule
  // must cost one empty cell, not the whole heatmap.
  for (let i = starts.length - 2; i >= 0; i--) {
    const next = starts[i + 1] as number;
    if ((starts[i] as number) > next) starts[i] = next;
  }
  return starts;
}

/**
 * The `days` local days ending with the one holding `now`, oldest first, in
 * the shape `query_heatmap` takes.
 */
export function heatmapDays(now: Date, days = 30): HeatmapDaySpec[] {
  const out: HeatmapDaySpec[] = [];
  for (let i = days - 1; i >= 0; i--) {
    const d = new Date(now.getFullYear(), now.getMonth(), now.getDate() - i);
    out.push({
      date: localDateIso(d),
      hour_starts: localHourStarts(d.getFullYear(), d.getMonth(), d.getDate()),
    });
  }
  return out;
}
