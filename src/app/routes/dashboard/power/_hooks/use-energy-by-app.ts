import { floorTo } from "@core/brush";
import { USAGE_BUCKET_MS } from "@core/generated/bindings";
import { historyKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useTransport } from "~/lib/transport-context";
import { useHost, useHostId } from "~/stores/host-store";

/**
 * `query_energy_by_app` over the chart window (D-093), ending at the start
 * of the 10 s bucket the newest sample is in, so the open bucket's partial
 * sums never show. The key moves once per bucket, which is when the table
 * reads again; the previous answer stays up meanwhile.
 */
export function useEnergyByApp(windowMs: number) {
  const transport = useTransport();
  const hostId = useHostId();
  const toMs = useHost((s) =>
    s.lastTsMs === null ? null : floorTo(s.lastTsMs, USAGE_BUCKET_MS)
  );
  const fromMs = toMs === null ? 0 : toMs - windowMs;
  return useQuery({
    queryKey: historyKeys.energyByApp(hostId, fromMs, toMs ?? 0),
    queryFn: () =>
      unwrap(transport.queryEnergyByApp(hostId, fromMs, toMs ?? 0)),
    enabled: toMs !== null,
    placeholderData: keepPreviousData,
  });
}
