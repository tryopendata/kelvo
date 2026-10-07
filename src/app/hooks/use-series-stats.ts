import type { TimeRange } from "@core/brush";
import type { SeriesStats } from "@core/generated/bindings";
import { historyKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useRangeQuery } from "~/lib/range-query";

/**
 * `query_series_stats` over `range` (D-099): average, peak and integral of
 * unlabelled host series from stored history. An answer that reaches the
 * end of `range` is final; one cut at now reads again every 10 s.
 */
export function useSeriesStats(
  metrics: readonly string[],
  range: TimeRange | null,
  { keepPrevious = false }: { keepPrevious?: boolean } = {}
) {
  return useRangeQuery<SeriesStats>(range, {
    key: (hostId, fromMs, toMs) =>
      historyKeys.seriesStats(hostId, metrics, fromMs, toMs),
    fetch: (transport, hostId, fromMs, toMs) =>
      unwrap(transport.querySeriesStats(hostId, [...metrics], fromMs, toMs)),
    isFinal: (d, toMs) => d !== undefined && d.to_ms >= toMs,
    keepPrevious,
  });
}
