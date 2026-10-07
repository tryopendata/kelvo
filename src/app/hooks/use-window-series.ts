import { gridIntervalMs, seriesWindow } from "@core/live-state";
import {
  bucketAverages,
  bucketIndex,
  liveBucketCount,
} from "@core/series-stats";
import { useMemo } from "react";
import { useHost, useHostStore } from "~/stores/host-store";

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
 */
export function useWindowSeries(
  keys: readonly string[],
  windowMs: number
): WindowSeries {
  const store = useHostStore();
  useHost((s) => s.rowsVersion);
  // Earlier history prepended, or the ring restarted: closed buckets change.
  const epoch = useHost((s) => s.rowsEpoch);
  const state = store.getState();
  const interval = gridIntervalMs(state.status);
  const factor = Math.ceil(windowMs / interval / MAX_CHART_POINTS);
  const bucketMs = factor * interval;
  const count = Math.ceil(windowMs / bucketMs);
  const last =
    factor > 1 && state.lastTsMs !== null
      ? bucketIndex(state.lastTsMs, bucketMs)
      : null;
  const keyList = keys.join("\n");
  const live = Math.min(count, liveBucketCount(state, keys, bucketMs));

  // Keyed by the newest bucket's index (and the epoch), not the store
  // snapshot: the final buckets only change when one of them moves. The
  // epoch is never negative; the test only makes it a dependency.
  const closed = useMemo(
    () =>
      epoch < 0 || last === null
        ? null
        : bucketAverages(
            store.getState(),
            keyList.split("\n"),
            bucketMs,
            last - count + 1,
            last - live
          ),
    [store, keyList, bucketMs, count, last, live, epoch]
  );

  if (factor <= 1) {
    const values: Record<string, (number | null)[]> = {};
    for (const k of keys) values[k] = seriesWindow(state, k, windowMs).values;
    return { values, tEndMs: state.lastTsMs ?? 0, intervalMs: interval };
  }

  const values: Record<string, (number | null)[]> = {};
  if (last === null || closed === null) {
    for (const k of keys) values[k] = new Array(count).fill(null);
    return { values, tEndMs: 0, intervalMs: bucketMs };
  }
  const open = bucketAverages(state, keys, bucketMs, last - live + 1, last);
  for (const k of keys) {
    const c = closed[k] ?? [];
    const o = open[k] ?? [];
    const row = new Array<number | null>(c.length + live);
    for (let i = 0; i < c.length; i++) row[i] = c[i] ?? null;
    for (let i = 0; i < live; i++) row[c.length + i] = o[i] ?? null;
    values[k] = row;
  }
  return { values, tEndMs: last * bucketMs, intervalMs: bucketMs };
}
