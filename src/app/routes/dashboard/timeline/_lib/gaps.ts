/**
 * The Timeline's gap bands (labels, bands and dedupe live in
 * @core/history-state) and the Sleep/Wake annotation row (plan 4.6, design-system.md
 * "Annotations": pills must not overlap; later ones move to a second row or
 * merge into a "+N").
 */

import { formatClock } from "@core/format";
import type { Gap } from "@core/generated/bindings";
import { dedupeGaps, type GapBandSpec, gapBands } from "@core/history-state";
import type { Span } from "./time";

/** A gap band on the Timeline; a module-scoped one covers that lane only. */
export type Band = GapBandSpec;

/** The Timeline's bands: clock times fit on 1h, durations on 24h. */
export function timelineBands(
  gaps: readonly Gap[],
  fromMs: number,
  toMs: number,
  span: Span
): Band[] {
  return gapBands(gaps, fromMs, toMs, {
    style: span === "1h" ? "clock" : "duration",
  });
}

export interface Marker {
  tMs: number;
  /**
   * `event`: a detector or alert pill, drawn ending at its time
   * so its dot sits there.
   */
  kind: "sleep" | "wake" | "event";
  label: string;
}

/** A Sleep marker at each sleep gap's start and a Wake marker at its end. */
export function sleepMarkers(
  gaps: readonly Gap[],
  fromMs: number,
  toMs: number
): Marker[] {
  const out: Marker[] = [];
  for (const g of dedupeGaps(gaps)) {
    if (g.reason !== "sleep") continue;
    if (g.start_ms >= fromMs && g.start_ms < toMs) {
      out.push({
        tMs: g.start_ms,
        kind: "sleep",
        label: `${formatClock(g.start_ms)} Sleep`,
      });
    }
    if (g.end_ms !== null && g.end_ms >= fromMs && g.end_ms < toMs) {
      out.push({
        tMs: g.end_ms,
        kind: "wake",
        label: `${formatClock(g.end_ms)} Wake`,
      });
    }
  }
  return out.sort((a, b) => a.tMs - b.tMs);
}

export interface PlacedMarker extends Marker {
  /** Left edge of the pill in px, inside the plot width. */
  leftPx: number;
  row: number;
  /** Markers merged into this one because every row was taken there. */
  more: number;
}

/** Space kept between two pills on one row. */
const MIN_GAP_PX = 8;

/**
 * Place markers left to right. A Sleep or Wake pill starts 4 px before its
 * time; an event pill ends 4 px after it. Each takes the first
 * row where it clears every pill already there; when no row has room it
 * merges into the nearest placed marker, which then shows "+N". A pill that
 * would run past either edge is pulled back inside. The result is in time
 * order.
 */
export function layoutMarkers(
  markers: readonly Marker[],
  fromMs: number,
  toMs: number,
  widthPx: number,
  measure: (m: Marker) => number,
  rows = 2
): PlacedMarker[] {
  const placed: PlacedMarker[] = [];
  const taken: [number, number][][] = Array.from({ length: rows }, () => []);
  const span = Math.max(1, toMs - fromMs);
  for (const m of markers) {
    const w = measure(m);
    const x = ((m.tMs - fromMs) / span) * widthPx;
    let left = m.kind === "event" ? x + 4 - w : x - 4;
    left = Math.max(0, Math.min(left, widthPx - w));
    const right = left + w;
    const row = taken.findIndex((spans) =>
      spans.every(([l, r]) => right + MIN_GAP_PX <= l || left >= r + MIN_GAP_PX)
    );
    if (row === -1 && placed.length > 0) {
      let near = placed[0] as PlacedMarker;
      for (const p of placed) {
        if (Math.abs(p.tMs - m.tMs) < Math.abs(near.tMs - m.tMs)) near = p;
      }
      near.more += 1;
      continue;
    }
    const r = row === -1 ? 0 : row;
    taken[r]?.push([left, right]);
    placed.push({ ...m, leftPx: left, row: r, more: 0 });
  }
  return placed.sort((a, b) => a.tMs - b.tMs);
}
