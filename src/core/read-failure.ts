/**
 * "Sensor read failed" (plan 4.17): a series the engine still lists in its
 * layout has no current value, while frames keep arriving. Rust nulls a
 * series' `held` once its last sample is older than its hold: the period
 * times `HOLD_FACTOR` (generated from the engine's `STALE_NUM / STALE_DEN`,
 * 5/2; `stale_ms` in `kelvo-engine/src/engine.rs`, D-047, D-090). So this is
 * a collector that failed on consecutive samples, not one slow tick, and the
 * client never times it itself. Paused and stale are their own states and
 * never count as a failed read.
 */
import { RING_SPAN_MS } from "@core/generated/bindings";
import type { HostLive } from "./live-state";

export interface ReadFailure {
  /** Time of the last row that measured the series, null if none in the ring. */
  lastGoodMs: number | null;
}

/**
 * Time of the newest row within one ring span that measured `key` (`NaN` in
 * its column is "not measured"). Scans back without allocating: it runs as a
 * 1 Hz selector while a series is failing.
 */
export function lastMeasuredMs(state: HostLive, key: string): number | null {
  const cols = state.columns;
  const newest = cols.lastTsMs();
  const col = cols.column(key);
  if (newest === null || !col) return null;
  const fromMs = newest - RING_SPAN_MS;
  for (let i = cols.length - 1; i >= 0; i--) {
    const tsMs = cols.tsAt(i);
    if (tsMs <= fromMs) break;
    if (!Number.isNaN(col[cols.slot(i)] as number)) return tsMs;
  }
  return null;
}

export function readFailure(state: HostLive, key: string): ReadFailure | null {
  if (state.layoutNo === null || state.lastTsMs === null) return null;
  if (state.stale || state.status?.paused) return null;
  const layout = state.layouts[state.layoutNo];
  if (!layout?.index.has(key)) return null;
  if (state.held[key] != null) return null;
  return { lastGoodMs: lastMeasuredMs(state, key) };
}

/**
 * Selector form: the last good time as a primitive (`undefined` when the
 * read is fine), so a component re-renders only when it changes.
 */
export function readFailureMs(
  state: HostLive,
  key: string
): number | null | undefined {
  const f = readFailure(state, key);
  return f === null ? undefined : f.lastGoodMs;
}
