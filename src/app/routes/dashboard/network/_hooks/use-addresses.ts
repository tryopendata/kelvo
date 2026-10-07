import { appKeys, hostKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/** A DHCP lease can change the address without a route change. */
const LOCAL_REFRESH_MS = 60_000;
/** The public lookup is a request to an outside service; ask rarely. */
const PUBLIC_STALE_MS = 10 * 60_000;

/**
 * The primary interface's addresses, read locally, and the public address
 * from an outside service (D-093). The local read is keyed by the primary
 * interface and refreshes each minute; the public one by the interface and
 * the local address it read, so joining another network asks again. The
 * public lookup waits for the local read, so a remote host (which has none
 * here) never shows this Mac's address. It runs only while the Network page
 * is mounted, at most every 10 minutes, and is not retried on failure.
 */
export function useAddresses(primary: string | null) {
  const transport = useTransport();
  const hostId = useHostId();
  const local = useQuery({
    queryKey: hostKeys.networkAddresses(hostId, primary),
    queryFn: () => unwrap(transport.getNetworkAddresses(hostId)),
    enabled: primary !== null,
    refetchInterval: LOCAL_REFRESH_MS,
  });
  const lan = local.data?.ipv4[0] ?? local.data?.ipv6[0] ?? null;
  const publicIp = useQuery({
    queryKey: appKeys.publicIp(primary, lan),
    queryFn: () => unwrap(transport.getPublicIp()),
    enabled: primary !== null && lan !== null,
    staleTime: PUBLIC_STALE_MS,
    gcTime: PUBLIC_STALE_MS,
    refetchInterval: PUBLIC_STALE_MS,
    retry: false,
  });
  return { lan, publicIp };
}
