import {
  formatClock,
  formatClockSeconds,
  formatHistoryTime,
} from "@core/format";
/**
 * Relative time label for a chart's x axis: "−60s", "−5m", "−24h". Uses the
 * true minus sign.
 */
export function agoLabel(ms: number): string {
  const s = Math.round(ms / 1000);
  if (s <= 0) return "now";
  if (s < 120) return `−${s}s`;
  const m = Math.round(s / 60);
  if (m < 120) return `−${m}m`;
  if (m >= 2880) return `−${Math.round(m / 1440)}d`;
  return `−${Math.round(m / 60)}h`;
}

/**
 * Evenly spaced x-axis labels for a window ending now: `count` labels from
 * "−window" to "now".
 */
export function windowTicks(windowMs: number, count: number): string[] {
  if (count < 2) return ["now"];
  return Array.from({ length: count }, (_, i) =>
    agoLabel((windowMs * (count - 1 - i)) / (count - 1))
  );
}

/** Gridlines of a fixed 0 to 100% chart, a quarter apart. */
export const PERCENT_GRID: readonly number[] = [25, 50, 75, 100];

/** Y tick labels of a fixed 0 to 100% chart, top down, without the unit. */
export const PERCENT_Y_TICKS: readonly { value: number; label: string }[] = [
  100, 75, 50, 25,
].map((v) => ({ value: v, label: String(v) }));

/**
 * Gridlines at half and full height of an autoscaled ceiling. With
 * `topLabel`, also the y ticks: the ceiling with its unit ("4W") and the
 * half as a bare number.
 */
export function ceilingAxis(
  ceiling: number,
  topLabel?: string
): {
  gridlines: number[];
  yTicks?: { value: number; label: string }[];
} {
  const half = ceiling / 2;
  const gridlines = [half, ceiling];
  if (topLabel === undefined) return { gridlines };
  return {
    gridlines,
    yTicks: [
      { value: ceiling, label: topLabel },
      { value: half, label: String(half) },
    ],
  };
}

/** Live views use the module-page relative axis; paused history names actual times. */
export function historyTicks(
  from: number,
  to: number,
  live: boolean,
  count = 5
): string[] {
  if (live) return windowTicks(to - from, count);
  const span = to - from;
  const format = (t: number) =>
    span >= 86_400_000
      ? formatHistoryTime(t, true)
      : span <= 300_000
        ? formatClockSeconds(t)
        : formatClock(t);
  return Array.from({ length: count }, (_, tick) =>
    format(from + ((to - from) * tick) / Math.max(1, count - 1))
  );
}
