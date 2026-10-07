/**
 * Clock and axis helpers for the Timeline, in the Mac's local time
 * ("Sun Oct 4 · 22:40", hour ticks, "14:02:00" in the tooltip).
 */
import { formatClock, monthName, weekdayName } from "@core/format";
import { HISTORY_TIERS } from "@core/generated/bindings";

/**
 * The presets, plus "6h": not a preset, only the window a heatmap cell older
 * than 7 days opens (its quarters are too wide for an hour). Live leaves it
 * for 24h.
 */
export type Span = "1h" | "6h" | "24h" | "7d" | "30d";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** The store's 10 s and 1-minute tiers (D-076, D-092). */
const [S10, M1] = HISTORY_TIERS;

export const SPAN_MS: Record<Span, number> = {
  "1h": HOUR,
  "6h": 6 * HOUR,
  "24h": DAY,
  "7d": 7 * DAY,
  "30d": 30 * DAY,
};

/**
 * Bucket widths a 7d or 30d range may be drawn at. Each from 15 minutes up
 * is a whole number of quarters, so a 15-minute page merges into it evenly;
 * the narrower ones come back as plain quarters from `tier_15m`.
 */
const LONG_BUCKETS_MS = [1, 2, 5, 10, 15, 30, 60, 120, 180, 360].map(
  (m) => m * MINUTE
);

/** Points per plot pixel a 7d or 30d range asks the store for. */
const POINTS_PER_PX = 2;

/**
 * The bucket the range is asked for at. 1h and 24h draw every bucket of
 * their tier (10 s, 1 min). 7d and 30d take the narrowest width in
 * `LONG_BUCKETS_MS` that keeps them at or under 2 points per plot pixel,
 * so the store merges buckets server-side and the page stays a few
 * thousand points. `plotPx` 0 (not measured yet) is treated as 1,000 px.
 */
export function requestBucketMs(span: Span, plotPx: number): number {
  if (span === "1h") return S10.bucket_ms;
  if (span === "6h" || span === "24h") return M1.bucket_ms;
  const points = POINTS_PER_PX * (plotPx > 0 ? plotPx : 1000);
  const want = SPAN_MS[span] / points;
  return (
    LONG_BUCKETS_MS.find((w) => w >= want) ??
    (LONG_BUCKETS_MS[LONG_BUCKETS_MS.length - 1] as number)
  );
}

export interface HistoryWindow {
  fromMs: number;
  toMs: number;
  maxPoints: number;
}

/**
 * The `query_history` window for a range ending at `toMs`: the start on a
 * bucket boundary and `max_points` one per bucket. The store merges whole
 * tier buckets from the one holding the start until the count fits, so
 * with the start aligned its points come back exactly `bucketMs` wide (or
 * one tier bucket, when that is wider) and on multiples of it.
 *
 * 1h and 24h ask for one bucket of slack before the range, so the window
 * drawn a bucket later than the read still has its first bucket. 7d does
 * not: Auto reads minutes only while the start is within a quarter of the
 * 7-day roll cut (D-076), and an aligned start is already up to a bucket
 * early. 30d, the same, for one rule.
 */
export function historyWindow(
  span: Span,
  toMs: number,
  bucketMs: number
): HistoryWindow {
  const slack = span === "1h" || span === "24h" ? bucketMs : 0;
  const fromMs =
    Math.floor((toMs - SPAN_MS[span]) / bucketMs) * bucketMs - slack;
  return {
    fromMs,
    toMs,
    maxPoints: Math.ceil((toMs - fromMs) / bucketMs),
  };
}

/**
 * The view's end after the back or forward button: one range length
 * earlier or later than the window drawn (`toMs`). Forward onto or past
 * `nowMs` returns `null`, back to following Live.
 */
export function stepRange(
  span: Span,
  toMs: number,
  direction: -1 | 1,
  nowMs: number
): number | null {
  const next = toMs + direction * SPAN_MS[span];
  return direction === 1 && next >= nowMs ? null : next;
}

/**
 * The view after the back or forward button. Like `stepRange`, but a 6h
 * window (a heatmap cell's) stepped forward onto now follows Live at 24h,
 * as the Live button does: 6h is not a preset.
 */
export function stepView(
  span: Span,
  toMs: number,
  direction: -1 | 1,
  nowMs: number
): { span: Span; endMs: number | null } {
  const endMs = stepRange(span, toMs, direction, nowMs);
  return endMs === null ? liveView(span) : { span, endMs };
}

/** Following Live: the same span, or 24h from the 6h window that has no preset. */
export function liveView(span: Span): { span: Span; endMs: null } {
  return { span: span === "6h" ? "24h" : span, endMs: null };
}

/** "10 s avg", "1 min avg", "30 min avg", "2 h avg": the tooltip's bucket width. */
export function resolutionLabel(bucketMs: number): string {
  if (bucketMs < MINUTE) return `${bucketMs / 1000} s avg`;
  if (bucketMs < HOUR) return `${bucketMs / MINUTE} min avg`;
  return `${bucketMs / HOUR} h avg`;
}

/** "Sun Oct 4". */
export function dayLabel(ms: number): string {
  const d = new Date(ms);
  return `${weekdayName(d)} ${monthName(d)} ${d.getDate()}`;
}

/** "Sun Oct 4 · 22:40". */
export function dayClock(ms: number): string {
  return `${dayLabel(ms)} · ${formatClock(ms)}`;
}

/** "Oct 4". */
export function monthDay(ms: number): string {
  const d = new Date(ms);
  return `${monthName(d)} ${d.getDate()}`;
}

const sameDay = (a: number, b: number) =>
  new Date(a).toDateString() === new Date(b).toDateString();

const LIVE_WORDS: Record<Span, string> = {
  "1h": "hour",
  "6h": "6 hours",
  "24h": "24 hours",
  "7d": "7 days",
  "30d": "30 days",
};

const PAST_WORDS: Record<Span, string> = {
  "1h": "One hour",
  "6h": "6 hours",
  "24h": "24 hours",
  "7d": "7 days",
  "30d": "30 days",
};

/**
 * The header subtitle, as a lead and the times in mono. Following Live it
 * names the end ("Last 24 hours, ending Sun Oct 4 ·
 * 22:40"). A window the user stepped back to gives both ends, the second
 * without its day when it is the same day: "7 days, Sun Sep 27 · 22:40 –
 * Sun Oct 4 · 22:40", "One hour, Sun Oct 4 · 21:40 – 22:40".
 */
export function rangeSubtitle(
  span: Span,
  fromMs: number,
  toMs: number,
  live: boolean
): { lead: string; times: string } {
  if (live) {
    return { lead: `Last ${LIVE_WORDS[span]}, ending `, times: dayClock(toMs) };
  }
  const end = sameDay(fromMs, toMs) ? formatClock(toMs) : dayClock(toMs);
  return {
    lead: `${PAST_WORDS[span]}, `,
    times: `${dayClock(fromMs)} – ${end}`,
  };
}

/**
 * A moment inside the range, short enough for a lane's sub line ("peak 71%
 * · 14:02"): the clock on 1h and 24h, weekday and clock on 7d to match its
 * day ticks ("Sat 14:20"), and the date alone on 30d ("Sep 27"), where a
 * point is 15 minutes or more wide.
 */
export function momentLabel(span: Span, ms: number): string {
  if (span === "30d") return monthDay(ms);
  if (span === "7d") return `${weekdayName(new Date(ms))} ${formatClock(ms)}`;
  return formatClock(ms);
}

/**
 * The label at the axis's right edge: "now" while following Live, else the
 * end time, with its date on the ranges that span days ("Oct 4 · 22:40").
 */
export function endLabel(span: Span, toMs: number, live: boolean): string {
  if (live) return "now";
  return span === "6h" || span === "7d" || span === "30d"
    ? `${monthDay(toMs)} · ${formatClock(toMs)}`
    : formatClock(toMs);
}

export interface AxisTick {
  tMs: number;
  label: string;
}

/**
 * Ticks for the shared x axis: every 10 minutes for 1h, every 3 hours on
 * the local hour for 24h, every hour for 6h, each local midnight for 7d ("Tue 29") and each
 * Monday's midnight for 30d ("Sep 28"). Ticks closer than a tenth of the
 * range to the right edge are dropped so they do not collide with the
 * "now" label.
 */
export function axisTicks(
  fromMs: number,
  toMs: number,
  span: Span
): AxisTick[] {
  const ticks: AxisTick[] = [];
  const edge = toMs - (toMs - fromMs) / 10;
  if (span === "7d" || span === "30d") {
    // Local midnights, stepped by calendar day so a DST change (a 23 or
    // 25 hour day) keeps them on midnight.
    const d = new Date(fromMs);
    d.setHours(0, 0, 0, 0);
    if (d.getTime() < fromMs) d.setDate(d.getDate() + 1);
    for (; d.getTime() < edge; d.setDate(d.getDate() + 1)) {
      const t = d.getTime();
      if (span === "7d") {
        ticks.push({ tMs: t, label: `${weekdayName(d)} ${d.getDate()}` });
      } else if (d.getDay() === 1) {
        ticks.push({ tMs: t, label: monthDay(t) });
      }
    }
    return ticks;
  }
  if (span === "6h" || span === "24h") {
    const every = span === "6h" ? 1 : 3;
    const d = new Date(fromMs);
    d.setMinutes(0, 0, 0);
    if (d.getTime() < fromMs) d.setHours(d.getHours() + 1);
    for (; d.getTime() < edge; d.setHours(d.getHours() + 1)) {
      if (d.getHours() % every === 0) {
        ticks.push({ tMs: d.getTime(), label: formatClock(d.getTime()) });
      }
    }
    return ticks;
  }
  const d = new Date(fromMs);
  d.setSeconds(0, 0);
  const rem = d.getMinutes() % 10;
  if (rem !== 0 || d.getTime() < fromMs)
    d.setMinutes(d.getMinutes() - rem + 10);
  for (; d.getTime() < edge; d.setMinutes(d.getMinutes() + 10)) {
    ticks.push({ tMs: d.getTime(), label: formatClock(d.getTime()) });
  }
  return ticks;
}

/** Minutes are kept this long; older ranges only have 15-minute buckets (D-076). */
const MINUTES_KEPT_MS = M1.kept_ms;

/**
 * The Timeline window a heatmap cell opens: the hour itself at 1h. 6 hours
 * centred on the cell instead when the hour is older than 7 days, since
 * only 15-minute buckets are left there and an hour would be four points,
 * or when the cell is longer than an hour (the 01:00 that happens twice on
 * a fall-back day), which 1h could not show whole. A window that reaches
 * now follows Live instead (`endMs: null`).
 */
export function heatmapCellView(
  hourStartMs: number,
  hourEndMs: number,
  nowMs: number
): { span: Span; endMs: number | null } {
  if (hourStartMs < nowMs - MINUTES_KEPT_MS || hourEndMs - hourStartMs > HOUR) {
    const mid = hourStartMs + (hourEndMs - hourStartMs) / 2;
    return { span: "6h", endMs: mid + SPAN_MS["6h"] / 2 };
  }
  return { span: "1h", endMs: hourEndMs >= nowMs ? null : hourEndMs };
}

/** Fraction of the range at `tMs`, clamped to [0, 1]. */
export function fractionAt(tMs: number, fromMs: number, toMs: number): number {
  const span = toMs - fromMs;
  if (span <= 0) return 0;
  return Math.min(1, Math.max(0, (tMs - fromMs) / span));
}
