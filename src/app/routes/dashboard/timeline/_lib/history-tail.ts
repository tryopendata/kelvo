import type { HistoryPage, TierRequest } from "@core/generated/bindings";
import { seriesKeyString } from "@core/series-key";

/** The part of a history read that a bucket closing asks for again. */
export interface TailRange {
  fromMs: number;
  toMs: number;
  tier: TierRequest;
  maxPoints: number;
}

/**
 * The read that brings a Live page up to `nowMs`: from the slot that was
 * open when the page was read (`readAtMs`) through the one open now, on the
 * page's own tier and slot grid, so the answer's slots are the page's
 * (`Reader::history` merges whole buckets from `from_ms`; whole slots in the
 * range keep its merge factor). `null` when the page cannot be extended that
 * way: a width of 0 (a peer that did not fill it), or a tier `query_history`
 * takes no fixed request for (M15); the caller reads the whole range again.
 */
export function tailRange(
  page: HistoryPage,
  readAtMs: number,
  nowMs: number
): TailRange | null {
  const w = page.bucket_ms;
  if (w <= 0 || (page.tier !== "s10" && page.tier !== "m1")) return null;
  // The slots start at the read's first bucket, not necessarily at a
  // multiple of the merged width: take their phase from a point.
  const t0 = page.series.find((s) => s.points.length > 0)?.points[0]?.t ?? 0;
  const phase = ((t0 % w) + w) % w;
  const fromMs = Math.floor((readAtMs - phase) / w) * w + phase;
  const slots = Math.max(1, Math.floor((nowMs - fromMs) / w) + 1);
  return {
    fromMs,
    toMs: fromMs + slots * w,
    tier: page.tier,
    maxPoints: slots,
  };
}

/**
 * `page` with everything from `fromMs` on replaced by `tail` (a read of
 * `[fromMs, …)`), and points and gaps that ended before `keepFromMs`, which
 * the window no longer shows, dropped. A series only the tail has is added.
 */
export function mergeTail(
  page: HistoryPage,
  tail: HistoryPage,
  fromMs: number,
  keepFromMs: number
): HistoryPage {
  const series = page.series.map((s) => ({
    ...s,
    points: s.points.filter((p) => p.t >= keepFromMs && p.t < fromMs),
  }));
  const index = new Map(series.map((s, i) => [seriesKeyString(s.key), i]));
  for (const t of tail.series) {
    const i = index.get(seriesKeyString(t.key));
    const s = i === undefined ? undefined : series[i];
    if (i === undefined || !s) {
      series.push(t);
      continue;
    }
    series[i] = {
      ...s,
      hold_ms: t.hold_ms,
      points: [...s.points, ...t.points],
    };
  }
  // The tail has every gap overlapping its range: keep the page's that end
  // before it.
  const gaps = [
    ...page.gaps.filter(
      (g) => g.end_ms !== null && g.end_ms <= fromMs && g.end_ms > keepFromMs
    ),
    ...tail.gaps,
  ];
  return { ...page, series, gaps };
}
