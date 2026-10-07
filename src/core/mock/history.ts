/**
 * `query_history` for the mock transport: deterministic buckets around each
 * series' sample value, with the scenario's gaps cut out (slots inside a gap
 * are absent, never zero).
 */
import {
  type BatteryHour,
  type Gap,
  HISTORY_RECENT_MS,
  HISTORY_TIERS,
  type HistoryPage,
  type HistoryPoint,
  type HistoryRequest,
  HOLD_FACTOR,
  type SeriesKey,
  type Tier,
} from "@core/generated/bindings";
import { seriesKeyString } from "@core/series-key";
import type { ScenarioFlags } from "./fixtures";
import type { SeriesSpec } from "./generator";
import { rng } from "./rng";

const MIN = 60_000;
const HOUR = 60 * MIN;

const DAY = 24 * HOUR;

/**
 * The 29 nights before the last one, for the 7d and 30d ranges: each a sleep
 * of 5 to 8 hours starting within an hour of the same clock time, all ending
 * more than a day ago so the 24h range keeps its one band. Night 20
 * is a weekend away (one 34-hour sleep covering night 19 too), and two
 * afternoons have multi-hour holes for other reasons: Kelvo not running
 * (day 3, inside the 7d range) and paused (day 12).
 */
function pastNights(nowMs: number): Gap[] {
  const r = rng(20261004);
  const gaps: Gap[] = [];
  for (let d = 29; d >= 1; d--) {
    const jitter = (r() - 0.5) * 2 * HOUR;
    const length = 5 * HOUR + r() * 3 * HOUR;
    if (d === 19) continue;
    const start =
      Math.round((nowMs - 18 * HOUR - d * DAY + jitter) / MIN) * MIN;
    const end = start + (d === 20 ? 34 * HOUR : Math.round(length / MIN) * MIN);
    gaps.push({ start_ms: start, end_ms: end, module: null, reason: "sleep" });
    const afternoon = start + 12 * HOUR;
    if (d === 3) {
      gaps.push({
        start_ms: afternoon,
        end_ms: afternoon + 4 * HOUR + 30 * MIN,
        module: null,
        reason: "app_not_running",
      });
    } else if (d === 12) {
      gaps.push({
        start_ms: afternoon,
        end_ms: afternoon + 3 * HOUR,
        module: null,
        reason: "paused",
      });
    }
  }
  return gaps;
}

/**
 * An overnight sleep ("Asleep 5h 15m · no samples"), and the nights before
 * it (`pastNights`). With the sleep-gap scenario, also a short one inside the last hour ("Asleep 11:02–11:31").
 */
export function mockGaps(flags: ScenarioFlags, nowMs: number): Gap[] {
  const gaps: Gap[] = [
    ...pastNights(nowMs),
    {
      start_ms: nowMs - 18 * HOUR,
      end_ms: nowMs - 18 * HOUR + 5 * HOUR + 15 * MIN,
      module: null,
      reason: "sleep",
    },
  ];
  if (flags.sleepGap) {
    gaps.push({
      start_ms: nowMs - 52 * MIN,
      end_ms: nowMs - 23 * MIN,
      module: null,
      reason: "sleep",
    });
  }
  if (flags.paused) {
    gaps.push({
      start_ms: nowMs - 2 * MIN,
      end_ms: null,
      module: null,
      reason: "paused",
    });
  }
  return gaps;
}

function matches(key: SeriesKey, sel: HistoryRequest["selectors"][number]) {
  if (key.metric !== sel.metric) return false;
  return sel.labels.every(([k, v]) =>
    key.labels.some(([kk, vv]) => kk === k && vv === v)
  );
}

function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return (h >>> 0) % 2147483646 || 1;
}

type StoredTier = Extract<Tier, "s10" | "m1" | "m15">;

const [S10, M1, M15] = HISTORY_TIERS;

const TIER_BASE: Record<StoredTier, number> = {
  s10: S10.bucket_ms,
  m1: M1.bucket_ms,
  m15: M15.bucket_ms,
};

/** The store's `m1_rolled` cut: the quarter at or before the minutes' window. */
function rollCut(nowMs: number): number {
  const q = M15.bucket_ms;
  return Math.floor((nowMs - M1.kept_ms) / q) * q;
}

/**
 * The store's `TierChoice::Auto` (D-076): S10 while a range no longer than
 * its 24 h retention starts inside it, M15 when the start is more than one
 * quarter before the roll cut, else M1. The quarter of slack is the
 * store's: a 7-day range ending now stays on minutes.
 */
function mockTier(req: HistoryRequest, nowMs: number): StoredTier {
  if (req.tier !== "auto") return req.tier;
  if (
    req.to_ms - req.from_ms <= S10.kept_ms &&
    req.from_ms >= nowMs - S10.kept_ms
  )
    return "s10";
  return rollCut(nowMs) - req.from_ms > M15.bucket_ms ? "m15" : "m1";
}

/**
 * `HistorySeries.hold_ms` (D-092): the hold of the slowest period the series
 * is sampled at (tray-only's idle cadence or the window's), or the page's
 * bucket if that is wider, as `LiveHub::history_hold_ms` computes it.
 */
function holdMs(spec: SeriesSpec, intervalMs: number, bucketMs: number) {
  const period = Math.max(spec.cadence, spec.idleCadence) * intervalMs;
  return Math.max((period * HOLD_FACTOR.num) / HOLD_FACTOR.den, bucketMs);
}

/** How far back the engine keeps rows the store may not have. */
const MOCK_RECENT_MS = HISTORY_RECENT_MS;

/**
 * History unavailable: Rust answers from the engine's recent rows
 * alone (`history_recent`), with no gaps. A fixed tier is read as asked;
 * Auto is 10 s buckets for a range S10 keeps, else minutes. The slots stay
 * on the range's own grid; only the rows the engine keeps have values.
 */
export function mockRecentHistory(
  req: HistoryRequest,
  specs: readonly SeriesSpec[],
  nowMs: number,
  intervalMs: number
): HistoryPage {
  const tier =
    req.tier !== "auto"
      ? req.tier
      : req.to_ms - req.from_ms <= S10.kept_ms
        ? "s10"
        : "m1";
  const page = mockHistory({ ...req, tier }, specs, [], nowMs, intervalMs);
  const kept = nowMs - MOCK_RECENT_MS;
  return {
    ...page,
    series: page.series.map((s) => ({
      ...s,
      points: s.points.filter((p) => p.t + page.bucket_ms > kept),
    })),
  };
}

export function mockHistory(
  req: HistoryRequest,
  specs: readonly SeriesSpec[],
  gaps: readonly Gap[],
  nowMs: number,
  intervalMs: number
): HistoryPage {
  const span = Math.max(0, req.to_ms - req.from_ms);
  const tier = mockTier(req, nowMs);
  const base = TIER_BASE[tier];
  // The store's merge (`Reader::history`): tier buckets from the one at or
  // before `from_ms`, merged in runs so at most `max_points` come back,
  // each point stamped with its run's first bucket.
  const first = Math.floor(req.from_ms / base) * base;
  const buckets = Math.ceil(span === 0 ? 0 : (req.to_ms - first) / base);
  const step =
    base * Math.max(1, Math.ceil(buckets / Math.max(1, req.max_points)));
  // Minutes older than the roll cut are gone (D-076): an `m1` read there
  // comes back empty, as the store's does.
  const floor = tier === "m1" ? rollCut(nowMs) : Number.NEGATIVE_INFINITY;
  const start = first + Math.max(0, Math.ceil((floor - first) / step)) * step;
  const inGap = (t: number) =>
    gaps.some((g) => t + step > g.start_ms && t < (g.end_ms ?? Infinity));

  // A wider bucket's min and max span more seconds, so they sit further from
  // its average: x1 at 10 s, about x1.8 at 1 min, x3 from 30 min, so the
  // envelope on 7d and 30d reads as one.
  const spread = Math.min(3, 1 + Math.log10(step / 10_000));

  const series = specs
    .filter((s) => req.selectors.some((sel) => matches(s.key, sel)))
    .map((s) => {
      const r = rng(hash(seriesKeyString(s.key)));
      const amp = s.noise || Math.abs(s.base) * 0.05;
      const points: HistoryPoint[] = [];
      for (let t = start; t < req.to_ms && t <= nowMs; t += step) {
        const wave = Math.sin(t / (3 * HOUR)) * 0.6 + (r() - 0.5) * 0.8;
        if (inGap(t)) continue;
        const avg = clamp(s.base + wave * amp, s);
        points.push({
          t,
          avg,
          min: clamp(avg - amp * 0.4 * spread * r(), s),
          max: clamp(avg + amp * 0.6 * spread * r(), s),
        });
      }
      return { key: s.key, points, hold_ms: holdMs(s, intervalMs, step) };
    });

  return {
    tier,
    bucket_ms: step,
    series,
    gaps: gaps.filter(
      (g) => g.start_ms < req.to_ms && (g.end_ms ?? Infinity) > req.from_ms
    ),
  };
}

function clamp(v: number, s: SeriesSpec): number {
  const c = Math.min(s.max ?? Infinity, Math.max(s.min ?? -Infinity, v));
  return Math.round(c * 1000) / 1000;
}

/** `MAX_BATTERY_HOURS` in `src-tauri/src/history.rs`: one per hour of 92 days. */
const MAX_BATTERY_HOURS = 92 * 24;

/**
 * `battery_hours`: minutes from `history`, cut at the local hour
 * boundaries Rust was given. An hour's charge is its last minute's, and it
 * charged when any minute did. `null` for an hour without minutes. A string
 * is the `invalid_argument` message.
 */
export function mockBatteryHours(
  host: string,
  hourStarts: readonly number[],
  history: (req: HistoryRequest) => HistoryPage
): BatteryHour[] | string {
  const hours = hourStarts.length - 1;
  if (hours < 1 || hours > MAX_BATTERY_HOURS) {
    return `${hourStarts.length} hour boundaries, expected 2 to ${MAX_BATTERY_HOURS + 1}`;
  }
  for (let i = 0; i < hours; i++) {
    if ((hourStarts[i + 1] as number) < (hourStarts[i] as number)) {
      return `hour boundaries go backwards at ${hourStarts[i]}`;
    }
  }
  const out: BatteryHour[] = hourStarts
    .slice(0, -1)
    .map((start_ms) => ({ start_ms, charge: null, charging: false }));
  const from = hourStarts[0] as number;
  const to = hourStarts[hours] as number;
  if (to <= from) return out;
  const page = history({
    host,
    selectors: [
      { metric: "battery.charge", labels: [] },
      { metric: "battery.charging", labels: [] },
    ],
    from_ms: from,
    to_ms: to,
    tier: "m1",
    max_points: Math.ceil((to - from) / M1.bucket_ms) + 2,
  });
  const cell = (t: number) =>
    hourStarts.findIndex(
      (start, i) => i < hours && t >= start && t < (hourStarts[i + 1] as number)
    );
  for (const s of page.series) {
    for (const p of s.points) {
      const hour = out[cell(p.t)];
      if (!hour) continue;
      if (s.key.metric === "battery.charge") {
        // Points come oldest first: the last one in the hour wins.
        if (p.avg !== null) hour.charge = p.avg;
      } else if ((p.max ?? 0) >= 0.5) {
        hour.charging = true;
      }
    }
  }
  return out;
}
