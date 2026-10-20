import type { TimeRange } from "@core/brush";
import type { UsageKey, UsageSeries } from "@core/generated/bindings";
import { historyKeys } from "@core/query-keys";
import { ceilTo } from "@core/time-grid";
import { unwrap } from "@core/transport";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { useRangeQuery } from "~/lib/range-query";

/** An answer whose buckets through the range's end no longer change. */
export const usageSeriesFinal = (data: UsageSeries | undefined, toMs: number) =>
  data !== undefined &&
  data.complete_to_ms !== null &&
  data.complete_to_ms >= ceilTo(toMs, data.bucket_ms);

/**
 * `query_usage_series` over `range`: the top `limit` apps by `by`, one value
 * per 10 s bucket (D-099). Local host only; a remote host's range is never
 * read. An answer whose buckets are complete is read once; one that reaches
 * time still being sampled reads again every 10 s.
 */
export function useUsageSeries(
  range: TimeRange | null,
  by: UsageKey,
  limit: number,
  { keepPrevious = false }: { keepPrevious?: boolean } = {}
) {
  const local = useIsLocalHost();
  return useRangeQuery<UsageSeries>(local ? range : null, {
    key: (hostId, fromMs, toMs) =>
      historyKeys.usageSeries(hostId, by, limit, fromMs, toMs),
    fetch: (transport, hostId, fromMs, toMs) =>
      unwrap(transport.queryUsageSeries(hostId, fromMs, toMs, by, limit)),
    isFinal: usageSeriesFinal,
    keepPrevious,
  });
}
