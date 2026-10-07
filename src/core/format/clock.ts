/**
 * Wall-clock and calendar labels in the viewer's time zone: "14:02",
 * "14:02:10", "Oct", "Sun", "Su".
 *
 * The tables are hand-built rather than `Intl.DateTimeFormat`, which follows
 * the WKWebView system locale and can give a 12-hour clock, "24:05" or a
 * translated month. Kelvo's labels are English and 24-hour everywhere.
 */
import { isPresent, type MaybeNumber, MISSING } from "./number";

export const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
] as const;

/** Indexed by `Date.getDay()`, Sunday first. */
export const WEEKDAYS = [
  "Sun",
  "Mon",
  "Tue",
  "Wed",
  "Thu",
  "Fri",
  "Sat",
] as const;

/** Two-letter weekdays for the heatmap's row labels, Sunday first. */
export const WEEKDAYS_2 = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"] as const;

/** "Oct" for the local month of `d`. */
export const monthName = (d: Date): string => MONTHS[d.getMonth()] as string;

/** "Sun" for the local weekday of `d`. */
export const weekdayName = (d: Date): string => WEEKDAYS[d.getDay()] as string;

/** Zero-padded to two digits: "05". */
export const pad2 = (n: number): string => String(n).padStart(2, "0");

/** Local "HH:MM": "14:02". */
export function formatClock(ms: number): string {
  const d = new Date(ms);
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}`;
}

/** Local wall-clock time with seconds, "14:02:10": a brushed range's ends. */
export function formatClockSeconds(ms: MaybeNumber): string {
  if (!isPresent(ms)) return MISSING;
  const d = new Date(ms);
  return `${pad2(d.getHours())}:${pad2(d.getMinutes())}:${pad2(d.getSeconds())}`;
}
