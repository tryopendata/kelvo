import type { TimeRange } from "@core/brush";
import {
  type HistoricalUsageMetric,
  USAGE_HISTORY_MAX_RANGE_MS,
} from "@core/generated/bindings";
import { historyKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import {
  useInfiniteQuery,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/** Fixed answers are cached until explicit refresh or history invalidation. Live
 * callers advance their bounds at the existing open-bucket edge, never at 1Hz.
 * Completeness cannot imply finality: late ownership can amend an older interval. */
export function useHistoricalUsage(
  range: TimeRange | null,
  by: HistoricalUsageMetric,
  { appNames = [] }: { appNames?: readonly string[] } = {}
) {
  const transport = useTransport();
  const host = useHostId();
  const from = range?.fromMs ?? 0;
  const to = range?.toMs ?? 0;
  return useQuery({
    queryKey: historyKeys.historicalUsage(host, from, to, by, appNames),
    queryFn: () =>
      unwrap(
        transport.queryHistoricalUsage({
          host,
          from_ms: from,
          to_ms: to,
          by,
          app_names: [...appNames],
        })
      ),
    enabled: range !== null && to - from <= USAGE_HISTORY_MAX_RANGE_MS,
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export function useHistoricalProcesses(
  range: TimeRange,
  by: HistoricalUsageMetric,
  appName: string
) {
  const transport = useTransport();
  const host = useHostId();
  const client = useQueryClient();
  const queryKey = historyKeys.usageProcesses(
    host,
    range.fromMs,
    range.toMs,
    by,
    appName
  );
  const query = useInfiniteQuery({
    queryKey,
    queryFn: ({ pageParam }) =>
      unwrap(
        transport.queryUsageProcesses({
          host,
          from_ms: range.fromMs,
          to_ms: range.toMs,
          by,
          app_name: appName,
          page_size: 100,
          cursor: pageParam,
        })
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor ?? undefined,
    staleTime: Number.POSITIVE_INFINITY,
  });
  return {
    ...query,
    restart: () => client.resetQueries({ queryKey, exact: true }),
  };
}
