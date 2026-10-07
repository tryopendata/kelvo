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

/** Wall-clock "HH:MM" in the viewer's zone, from a ms epoch. */
export function clockLabel(tsMs: number): string {
  const d = new Date(tsMs);
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}
