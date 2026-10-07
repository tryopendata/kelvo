import type { Gap, HistoryPage } from "@core/generated/bindings";

/**
 * The largest bucket max over a history page. Totals are their own series
 * (`disk.read_total`, D-092), so nothing is summed here. Null when nothing
 * was recorded.
 */
export function pageMax(page: HistoryPage): number | null {
  let out: number | null = null;
  for (const s of page.series) {
    for (const p of s.points) {
      if (p.max !== null && (out === null || p.max > out)) out = p.max;
    }
  }
  return out;
}

/** The scale for an Overview bar: the 24 h maximum, never below `floor`. */
export function scaleFrom(max: number | null | undefined, floor: number) {
  return Math.max(floor, max ?? 0);
}

/** When the Mac last woke: the end of the latest closed sleep gap. */
export function lastWakeMs(gaps: readonly Gap[]): number | null {
  let out: number | null = null;
  for (const g of gaps) {
    if (g.reason !== "sleep" || g.end_ms === null) continue;
    if (out === null || g.end_ms > out) out = g.end_ms;
  }
  return out;
}
