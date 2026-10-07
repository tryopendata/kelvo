import type { LayoutInfo } from "@core/live-state";
import {
  bucketAverages,
  bucketIndex,
  liveBucketCount,
  type SeriesStat,
  seriesStats,
} from "@core/series-stats";
import { useMemo } from "react";
import { useShallow } from "zustand/react/shallow";
import { useHost, useHostStore } from "~/stores/host-store";

/**
 * Current values for an arbitrary list of series keys (display form), `null`
 * where a series has no current value. Shallow-compared, so the caller
 * re-renders only when one of these values changes.
 */
export function useHeld(
  keys: readonly string[]
): Record<string, number | null> {
  return useHost(
    useShallow((s) => {
      const out: Record<string, number | null> = {};
      for (const k of keys) out[k] = s.held[k] ?? null;
      return out;
    })
  );
}

/**
 * The current layout (series in value order, keys per metric). The object is
 * replaced only when a new layout arrives, so it is a stable dependency.
 */
export function useLayout(): LayoutInfo | null {
  return useHost((s) =>
    s.layoutNo === null ? null : (s.layouts[s.layoutNo] ?? null)
  );
}

/**
 * avg, min and max of each key over the last `windowMs` of the ring.
 * Recomputed when rows are appended (once per tick).
 */
export function useRingStats(
  keys: readonly string[],
  windowMs: number
): Record<string, SeriesStat | null> {
  const store = useHostStore();
  useHost((s) => s.rowsVersion);
  return seriesStats(store.getState(), keys, windowMs);
}

/**
 * Averages per key in `count` wall-clock-aligned buckets ending with the one
 * that holds the newest row. Buckets that can no longer change are computed
 * only when a bucket closes; the open one, and the closed ones a later sample
 * can still fill back into within its hold (`liveBucketCount`), every tick. `endMs` is the newest bucket's
 * start (null before the first row).
 */
export function useRingBuckets(
  keys: readonly string[],
  bucketMs: number,
  count: number
): { values: Record<string, (number | null)[]>; endMs: number | null } {
  const store = useHostStore();
  useHost((s) => s.rowsVersion);
  // Earlier history prepended, or the ring restarted: closed buckets change.
  const epoch = useHost((s) => s.rowsEpoch);
  const state = store.getState();
  const lastTs = state.lastTsMs;
  const last = lastTs === null ? null : bucketIndex(lastTs, bucketMs);
  const keyList = keys.join("\n");
  const live = Math.min(count, liveBucketCount(state, keys, bucketMs));

  // Keyed by the closed bucket index, the epoch and the key list, not the
  // store snapshot. The epoch is never negative; the test only makes it a
  // dependency.
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
  if (last === null || closed === null) return { values: {}, endMs: null };
  const open = bucketAverages(state, keys, bucketMs, last - live + 1, last);
  const values: Record<string, (number | null)[]> = {};
  for (const k of keys) {
    values[k] = [...(closed[k] ?? []), ...(open[k] ?? [])];
  }
  return { values, endMs: last * bucketMs };
}
