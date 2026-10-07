import { isPresent, type MaybeNumber, MISSING } from "./number";

const MINUTE = 60_000;
const UNITS = [
  { suffix: "d", minutes: 24 * 60 },
  { suffix: "h", minutes: 60 },
  { suffix: "m", minutes: 1 },
] as const;

export interface DurationOptions {
  /** How many units to show from the largest non-zero one. Default 2. */
  parts?: number;
}

/**
 * Elapsed time, truncated (an uptime of 3d 4h 59m is "3d 4h", never "3d 5h"):
 * "3d 4h" (popover uptime), "5h 15m" (sleep gap), "3d 4h 12m" with
 * `parts: 3` (machine header). Under an hour it reads as prose, "4 min",
 * as in "First 4 min"; under a minute, "<1 min". Zero units after
 * the first are dropped ("2d", not "2d 0h").
 */
export function formatDuration(
  ms: MaybeNumber,
  { parts = 2 }: DurationOptions = {}
): string {
  if (!isPresent(ms) || ms < 0) return MISSING;
  let rest = Math.floor(ms / MINUTE);
  if (rest < 1) return "<1 min";
  if (rest < 60) return `${rest} min`;

  const out: string[] = [];
  let started = false;
  let taken = 0;
  for (const u of UNITS) {
    const n = Math.floor(rest / u.minutes);
    rest -= n * u.minutes;
    if (!started && n === 0) continue;
    started = true;
    taken += 1;
    if (n > 0) out.push(`${n}${u.suffix}`);
    if (taken === parts) break;
  }
  return out.join(" ");
}

/**
 * Hours and minutes on a clock face, "6:12": battery time remaining. Hours are
 * not capped at 24 and minutes are truncated.
 */
export function formatHoursMinutes(ms: MaybeNumber): string {
  if (!isPresent(ms) || ms < 0) return MISSING;
  const total = Math.floor(ms / MINUTE);
  const h = Math.floor(total / 60);
  const m = total % 60;
  return `${h}:${String(m).padStart(2, "0")}`;
}

/** Local wall-clock time with seconds, "14:02:10": a brushed range's ends. */
export function formatClockSeconds(ms: MaybeNumber): string {
  if (!isPresent(ms)) return MISSING;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

/**
 * A selected span in the Apps title: "90 s" under two minutes, then "2 min",
 * "2 min 30 s", and "1 h 5 min" from an hour.
 */
export function formatSpan(ms: MaybeNumber): string {
  if (!isPresent(ms) || ms < 0) return MISSING;
  const s = Math.round(ms / 1000);
  if (s < 120) return `${s} s`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const rest = s % 60;
  if (h > 0) return m > 0 ? `${h} h ${m} min` : `${h} h`;
  return rest > 0 ? `${m} min ${rest} s` : `${m} min`;
}
