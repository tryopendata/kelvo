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

/** Last ring row that measured `key`, newest first. */
export function lastMeasuredMs(state: HostLive, key: string): number | null {
  const last = state.rows.last();
  if (!last) return null;
  const rows = state.rows.since(last.tsMs - RING_SPAN_MS);
  for (let i = rows.length - 1; i >= 0; i--) {
    const row = rows[i];
    if (!row) continue;
    const idx = state.layouts[row.layoutNo]?.index.get(key);
    if (idx !== undefined && row.values[idx] != null) return row.tsMs;
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
