import { BRUSH_STEP_MS, type TimeRange } from "@core/brush";
import { historyKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/**
 * `query_network_totals` over `range`: interface bytes from the stored
 * totals, in every edition. An answer that reaches the end of `range` is
 * final and read once; one cut at now (the range reaches the open bucket)
 * reads again every 10 s. `keepPrevious` holds the last answer while the key
 * moves (the whole-window view, whose key advances every 10 s).
 */
export function useNetworkTotals(
  range: TimeRange | null,
  { keepPrevious = false }: { keepPrevious?: boolean } = {}
) {
  const transport = useTransport();
  const hostId = useHostId();
  const fromMs = range?.fromMs ?? 0;
  const toMs = range?.toMs ?? 0;
  return useQuery({
    queryKey: historyKeys.networkTotals(hostId, fromMs, toMs),
    queryFn: () => unwrap(transport.queryNetworkTotals(hostId, fromMs, toMs)),
    enabled: range !== null,
    staleTime: (q) =>
      q.state.data && q.state.data.to_ms >= toMs ? Number.POSITIVE_INFINITY : 0,
    refetchInterval: (q) =>
      q.state.status === "error" || (q.state.data && q.state.data.to_ms >= toMs)
        ? false
        : BRUSH_STEP_MS,
    placeholderData: keepPrevious ? keepPreviousData : undefined,
  });
}
