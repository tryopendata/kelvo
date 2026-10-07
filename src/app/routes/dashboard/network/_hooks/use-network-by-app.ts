import { BRUSH_STEP_MS, floorTo, type TimeRange } from "@core/brush";
import type { NetworkByApp } from "@core/generated/bindings";
import { historyKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useTransport } from "~/lib/transport-context";
import { useHost, useHostId } from "~/stores/host-store";

/**
 * Start of the 10 s bucket the newest sample is in, or null before the
 * first sample. Changes once every 10 s, so a component reading it
 * re-renders at that cadence, not at 1 Hz. The engine may still hold the
 * bucket before it open (see `useCompleteEdge`).
 */
export function useOpenEdge(): number | null {
  return useHost((s) => (s.lastTsMs === null ? null : floorTo(s.lastTsMs)));
}

/**
 * An answer is final once every bucket in it is complete: both the per-app
 * and the interface streams have reported past it (D-089). Until then its
 * buckets still grow, so it is read again.
 */
export const isFinal = (data: NetworkByApp | undefined) =>
  data !== undefined && data.complete_to_ms >= data.to_ms;

/**
 * `query_network_by_app` over `range` (D-089). A final answer never
 * changes, so it is read once; one that reaches an open bucket reads again
 * every `pollMs(answer)` ms (10 s by default) until its buckets close. `keepPrevious`
 * holds the last answer while the key moves (the whole-window view, whose
 * key advances every 10 s), so the table does not blank between reads.
 */
export function useNetworkByApp(
  range: TimeRange | null,
  {
    keepPrevious = false,
    pollMs = () => BRUSH_STEP_MS,
  }: {
    keepPrevious?: boolean;
    /** `reads` counts answers under the current key. */
    pollMs?: (data: NetworkByApp | undefined, reads: number) => number;
  } = {}
) {
  const transport = useTransport();
  const hostId = useHostId();
  return useQuery({
    queryKey: historyKeys.networkByApp(
      hostId,
      range?.fromMs ?? 0,
      range?.toMs ?? 0
    ),
    queryFn: () =>
      unwrap(
        transport.queryNetworkByApp(
          hostId,
          range?.fromMs ?? 0,
          range?.toMs ?? 0
        )
      ),
    enabled: range !== null,
    staleTime: (q) => (isFinal(q.state.data) ? Number.POSITIVE_INFINITY : 0),
    refetchInterval: (q) =>
      q.state.status === "error" || isFinal(q.state.data)
        ? false
        : pollMs(q.state.data, q.state.dataUpdateCount),
    placeholderData: keepPrevious ? keepPreviousData : undefined,
  });
}

/**
 * How soon a probe of the bucket before the open edge is read again while
 * only that bucket is still open. The per-app stream reports a few seconds
 * past each edge (its own 10 s phase with no process view), so waiting for
 * the next edge would leave "now" a bucket staler than it has to be. When
 * the answer is open further back than that bucket, the stream has stalled
 * and buckets close only by grace: the probe reads every 10 s instead.
 * The fast phase also ends after `PROBE_FAST_READS` answers under one key,
 * so a bucket that stays open without the edge moving (sampling paused)
 * is read every 10 s, not every 2 s.
 */
export const PROBE_POLL_MS = 2_000;
export const PROBE_FAST_READS = 5;

/**
 * Where the engine's complete buckets end, from the bucket before the open
 * edge: the open edge itself once that bucket is complete, earlier while
 * the per-app stream has not reported past it. Null until the first answer.
 * A failed probe falls back to the bucket before the open edge, so the
 * totals read runs and reports the failure itself rather than leave the
 * card loading.
 */
export function useCompleteEdge(openEdgeMs: number | null): number | null {
  const probe = useNetworkByApp(
    openEdgeMs === null
      ? null
      : { fromMs: openEdgeMs - BRUSH_STEP_MS, toMs: openEdgeMs },
    {
      keepPrevious: true,
      pollMs: (data, reads) =>
        openEdgeMs === null ||
        reads >= PROBE_FAST_READS ||
        (data !== undefined && data.complete_to_ms < openEdgeMs - BRUSH_STEP_MS)
          ? BRUSH_STEP_MS
          : PROBE_POLL_MS,
    }
  );
  if (openEdgeMs === null) return null;
  if (probe.data) return Math.min(probe.data.complete_to_ms, openEdgeMs);
  return probe.isError ? openEdgeMs - BRUSH_STEP_MS : null;
}

/**
 * The latest complete 10 s bucket per app, for the "now" column: the one
 * ending at the complete edge. Read through the same command as the totals,
 * so its names match the table's rows; the key moves every 10 s.
 */
export function useLatestBucket(completeEdgeMs: number | null) {
  return useNetworkByApp(
    completeEdgeMs === null
      ? null
      : { fromMs: completeEdgeMs - BRUSH_STEP_MS, toMs: completeEdgeMs },
    { keepPrevious: true }
  );
}
