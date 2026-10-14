import type { RemoteHostRecord } from "@core/generated/bindings";
import { hostKeys } from "@core/query-keys";
import { useQuery } from "@tanstack/react-query";
import { useTransport } from "~/lib/transport-context";

/** Connection metadata only; metric samples arrive on the live channel. */
export function useRemoteHosts<T = RemoteHostRecord[]>(
  select?: (hosts: RemoteHostRecord[]) => T
) {
  const transport = useTransport();
  return useQuery({
    ...(select ? { select } : {}),
    queryKey: hostKeys.remote(),
    queryFn: () => transport.listRemoteHosts(),
    refetchInterval: transport.windowLabel() === "dashboard" ? 5000 : false,
    staleTime: 4000,
  });
}
