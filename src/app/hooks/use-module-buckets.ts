import { useHistorySeries } from "~/hooks/use-history-series";
import { useRingBuckets } from "~/hooks/use-ring";

/** Heatmaps and mirrored charts share the same live/retained range switch. */
export function useModuleBuckets(
  keys: readonly string[],
  bucketMs: number,
  count: number,
  windowMs: number
) {
  // Cells and bars show Rust's average of measured time; gap bands overlay the missing time.
  const history = useHistorySeries(keys, windowMs, count, false);
  const ring = useRingBuckets(keys, bucketMs, count, {
    enabled: !history.enabled,
  });
  return {
    ...(history.enabled
      ? (history.grid ?? { values: {}, endMs: null, bucketMs })
      : { ...ring, bucketMs }),
    loading: history.loading,
    error: history.error,
  };
}
