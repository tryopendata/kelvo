import type { TimeRange } from "@core/brush";
import { historyKeys, hostKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useOpenEdge } from "~/hooks/use-range-scope";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

export function useDiskDevices(enabled = true) {
  const transport = useTransport();
  const host = useHostId();
  return useQuery({
    queryKey: hostKeys.diskIoDevices(host),
    enabled,
    queryFn: () => unwrap(transport.listDiskIoDevices(host)),
    staleTime: 30_000,
    refetchInterval: 30_000,
  });
}

/** One weighted query for both plots. Never average client-side latency ratios. */
export function useDiskIo(
  device: string | null,
  range: TimeRange | null,
  maxPoints = 120,
  hold = false
) {
  const edge = useOpenEdge();
  const transport = useTransport();
  const host = useHostId();
  const from = range?.fromMs ?? 0;
  const to = range?.toMs ?? 0;
  return useQuery({
    queryKey: historyKeys.diskIo(host, device, from, to, maxPoints),
    queryFn: () =>
      unwrap(
        transport.queryDiskIo({
          host,
          device,
          from_ms: from,
          to_ms: to,
          max_points: maxPoints,
        })
      ),
    enabled: range !== null,
    staleTime: edge !== null && to <= edge ? Infinity : 0,
    refetchInterval: edge !== null && to <= edge ? false : 10_000,
    placeholderData: (previous, query) =>
      hold &&
      query?.queryKey[1] === host &&
      query.queryKey[3] === device &&
      Number(query.queryKey[5]) - Number(query.queryKey[4]) === to - from
        ? previous
        : undefined,
  });
}
