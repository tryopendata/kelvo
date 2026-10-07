import type { HostLive } from "@core/live-state";
import { windowMean } from "@core/series-stats";
import { INTERVALS_MS } from "@core/settings-patch";

/** The overhead sentence averages Kelvo's own CPU over this window (plan 4.15). */
export const OVERHEAD_WINDOW_MS = 10 * 60_000;

/** Readings under the current sampling setup before the sentence quotes one. */
export const MIN_READINGS = 2;

/**
 * The longest span one `self.cpu` reading covers: the slowest interval. A
 * reading that far before the window can still be the first after a change.
 */
const LONGEST_READING_MS = Math.max(...INTERVALS_MS);

const KEY = "self.cpu";

/**
 * Mean of `self.cpu` over the last ten minutes of the live ring, counting only
 * readings taken under the current sampling setup (`statusSinceMs`), rounded
 * to 0.1 so a selector returning it changes only when the sentence would.
 *
 * Each reading is Kelvo's CPU over the span since the previous one, so the
 * first reading after a change still covers time under the old setup and is
 * skipped, even when it lands just inside the window. The mean is
 * `windowMean` (as the popover footer takes it) over the window cut to start
 * after that reading, so each reading weighs by the seconds it averages.
 * Until `MIN_READINGS` more arrive the result is `"measuring"`. Paused or
 * display-idle, nothing is being measured, and with no reading at all there
 * is nothing to say: both are null, and the sentence is hidden rather than
 * claiming 0%.
 */
export function selfCpuAverage(
  state: Pick<
    HostLive,
    "columns" | "layouts" | "lastTsMs" | "statusSinceMs" | "status"
  >
): number | "measuring" | null {
  const end = state.lastTsMs;
  if (end === null) return null;
  if (state.status?.paused || state.status?.display_idle) return null;
  const start = end - OVERHEAD_WINDOW_MS;
  const since = state.statusSinceMs;
  // Read back far enough to find the reading that spans a change near the
  // window's start; only readings inside the window count.
  const from =
    since === null
      ? start
      : Math.min(start, Math.max(since, start - LONGEST_READING_MS));
  const cols = state.columns;
  const col = cols.column(KEY);
  // Readings from here on are under the current setup.
  let after = start;
  let spanning = true;
  let n = 0;
  let seen = false;
  for (
    let i = col ? cols.firstAfter(from) : cols.length;
    i < cols.length;
    i++
  ) {
    const v = (col as Float32Array)[cols.slot(i)] as number;
    if (!Number.isFinite(v)) continue;
    const tsMs = cols.tsAt(i);
    if (tsMs > start) seen = true;
    if (since === null || tsMs < since) continue;
    if (spanning) {
      spanning = false;
      if (since >= from) {
        after = Math.max(after, tsMs);
        continue;
      }
    }
    if (tsMs > start) n += 1;
  }
  if (n < MIN_READINGS) return seen || since === null ? "measuring" : null;
  const mean = windowMean(state, KEY, end - after);
  return mean === null ? null : Math.round(mean * 10) / 10;
}

/** "0.5s", "1s", "2s", "5s", "10s", "30s", "60s". */
export function intervalLabel(ms: number): string {
  return `${ms / 1000}s`;
}
