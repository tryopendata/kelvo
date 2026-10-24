import { HISTORY_TIERS, RING_SPAN_MS } from "@core/generated/bindings";
import { historyGrid } from "@core/history-grid";
import { historyKeys } from "@core/query-keys";
import { seriesKeyString } from "@core/series-key";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { useHostClock } from "~/lib/host-clock";
import { useTransport } from "~/lib/transport-context";
import { useModuleRange } from "~/stores/chart-range";
import { useHost, useHostId } from "~/stores/host-store";

/** Bounded retained-history reads. Relative ranges refresh once per displayed bucket. */
export function useHistorySeries(
  keys: readonly string[],
  windowMs: number,
  points: number,
  breakAtGaps = true
) {
  const range = useModuleRange();
  const end = range?.endMs ?? null;
  const enabled = windowMs > RING_SPAN_MS || end !== null;
  const clock = useHostClock();
  const hostId = useHostId();
  const transport = useTransport();
  const width = Math.max(
    HISTORY_TIERS[0].bucket_ms,
    Math.min(60_000, Math.ceil(windowMs / points))
  );
  const edge = useHost((s) =>
    enabled && end === null && s.lastTsMs !== null
      ? Math.floor(s.lastTsMs / width) * width
      : null
  );
  // A disabled history observer must not rotate cache entries as the live clock moves.
  const toMs = enabled
    ? (end ?? edge ?? Math.floor(clock() / width) * width)
    : 0;
  const keyList = keys.join("\n");
  const stableKeys = useMemo(
    () => (keyList ? keyList.split("\n") : []),
    [keyList]
  );
  const layout = useHost((s) =>
    s.layoutNo === null ? null : s.layouts[s.layoutNo]
  );
  const query = useQuery({
    queryKey: historyKeys.moduleSeries(
      hostId,
      stableKeys,
      windowMs,
      toMs,
      points,
      end !== null
    ),
    queryFn: () => {
      const selectors = stableKeys.map((key) => {
        const known = layout?.series.find(
          (series) => seriesKeyString(series) === key
        );
        // Unlabelled historic series may be absent from the current layout.
        return known ?? { metric: key, labels: [] };
      });
      return unwrap(
        transport.queryHistory({
          host: hostId,
          selectors,
          from_ms: toMs - windowMs,
          to_ms: toMs,
          tier: "auto",
          max_points: points,
        })
      );
    },
    enabled: enabled && stableKeys.length > 0,
    gcTime: 60_000,
    placeholderData: (previous, query) =>
      end === null &&
      query?.queryKey[1] === hostId &&
      JSON.stringify(query.queryKey[3]) === JSON.stringify(stableKeys) &&
      query.queryKey[4] === windowMs &&
      query.queryKey[6] === points &&
      query.queryKey[7] === false
        ? previous
        : undefined,
  });
  const grid = useMemo(
    () =>
      query.data
        ? historyGrid(
            query.data,
            stableKeys,
            toMs - windowMs,
            toMs,
            breakAtGaps
          )
        : null,
    [query.data, stableKeys, windowMs, toMs, breakAtGaps]
  );
  return {
    enabled,
    grid,
    page: query.data,
    loading: enabled && stableKeys.length > 0 && query.isPending,
    error: enabled && query.isError,
  };
}
