import type { TimeRange } from "@core/brush";
import type { UsageByApp, UsageKey } from "@core/generated/bindings";
import { historyKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { usageFinal } from "@core/usage-rows";
import { useQuery } from "@tanstack/react-query";
import { rangeQueryOptions } from "~/lib/range-query";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/** Apps a usage table asks for: every app a search could find, in practice. */
export const USAGE_LIMIT = 200;

/**
 * `query_usage_by_app` over `range`, the top apps by `by` (D-099). An answer
 * whose buckets are complete is read once; one that reaches time still
 * being sampled reads again every 10 s.
 */
export function useUsageByApp(
  range: TimeRange | null,
  by: UsageKey,
  { keepPrevious = false }: { keepPrevious?: boolean } = {}
) {
  const transport = useTransport();
  const hostId = useHostId();
  const fromMs = range?.fromMs ?? 0;
  const toMs = range?.toMs ?? 0;
  return useQuery({
    queryKey: historyKeys.usageByApp(hostId, by, USAGE_LIMIT, fromMs, toMs),
    queryFn: () =>
      unwrap(transport.queryUsageByApp(hostId, fromMs, toMs, by, USAGE_LIMIT)),
    enabled: range !== null,
    ...rangeQueryOptions<UsageByApp>(usageFinal, keepPrevious),
  });
}
