import { hostKeys } from "@core/query-keys";
import { useQuery } from "@tanstack/react-query";
import { useTransport } from "~/lib/transport-context";

/** Connection metadata only; metric samples arrive on the live channel. */
export function useRemoteHosts() {
  const transport = useTransport();
  return useQuery({
    queryKey: hostKeys.remote(),
    queryFn: () => transport.listRemoteHosts(),
    refetchInterval: transport.windowLabel() === "dashboard" ? 5000 : false,
    staleTime: 4000,
  });
}
