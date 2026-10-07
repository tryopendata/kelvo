import type { TimeRange } from "@core/brush";
import type { SeriesStats } from "@core/generated/bindings";
import { historyKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { rangeQueryOptions } from "~/lib/range-query";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

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
  const transport = useTransport();
  const hostId = useHostId();
  const fromMs = range?.fromMs ?? 0;
  const toMs = range?.toMs ?? 0;
  return useQuery({
    queryKey: historyKeys.seriesStats(hostId, metrics, fromMs, toMs),
    queryFn: () =>
      unwrap(transport.querySeriesStats(hostId, [...metrics], fromMs, toMs)),
    enabled: range !== null,
    ...rangeQueryOptions<SeriesStats>(
      (d) => d !== undefined && d.to_ms >= toMs,
      keepPrevious
    ),
  });
}
