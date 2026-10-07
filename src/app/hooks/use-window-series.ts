import { brushBucketMs } from "@core/brush";
import { gridIntervalMs, seriesWindow } from "@core/live-state";
import { useHostStore } from "~/stores/host-store";
import { useRingBuckets } from "./use-ring";

/**
 * At most this many points per series: two per pixel of a chart about 600 px
 * wide (plan 4.7: 15m and 1h are downsampled to 2 points per pixel).
 */
export const MAX_CHART_POINTS = 1200;

export interface WindowSeries {
  /** Per key, oldest first, `intervalMs` apart; `null` is a gap. */
  values: Record<string, (number | null)[]>;
  tEndMs: number;
  intervalMs: number;
}

/**
 * Several series over the last `windowMs` of the ring for one live chart.
 * Up to `MAX_CHART_POINTS` slots it is the raw interval grid, read from the
 * typed series columns; beyond that the slots are averaged into buckets
 * aligned to wall-clock multiples, so a bucket keeps its value as the window
 * slides and the chart scrolls one bucket at a time. Buckets that can no
 * longer change are computed once, when the newest bucket moves; each tick
 * recomputes the open one and those a later sample can still fill back into
 * within its hold (`liveBucketCount`). Re-renders once per tick.
 *
 * With `brush` (D-089) the points are a width that divides 10 s or is a
 * multiple of it (3 s at 1h becomes 5 s), so a selection snapped to 10 s
 * buckets lines up with them.
 */
export function useWindowSeries(
  keys: readonly string[],
  windowMs: number,
  { brush = false }: { brush?: boolean } = {}
): WindowSeries {
  const store = useHostStore();
  const interval = gridIntervalMs(store.getState().status);
  const plainMs = Math.ceil(windowMs / interval / MAX_CHART_POINTS) * interval;
  const bucketMs = brush ? brushBucketMs(plainMs, interval) : plainMs;
  const factor = bucketMs / interval;
  const count = Math.ceil(windowMs / bucketMs);
  // Subscribes to the tick; the bucket work is skipped on the raw grid.
  const buckets = useRingBuckets(keys, bucketMs, count, {
    enabled: factor > 1,
  });
  const state = store.getState();

  if (factor <= 1) {
    const values: Record<string, (number | null)[]> = {};
    for (const k of keys) values[k] = seriesWindow(state, k, windowMs).values;
    return { values, tEndMs: state.lastTsMs ?? 0, intervalMs: interval };
  }

  if (buckets.endMs === null) {
    const values: Record<string, (number | null)[]> = {};
    for (const k of keys) values[k] = new Array(count).fill(null);
    return { values, tEndMs: 0, intervalMs: bucketMs };
  }
  return {
    values: buckets.values,
    tEndMs: buckets.endMs,
    intervalMs: bucketMs,
  };
}
