import { appKeys, hostKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";

/**
 * A DHCP lease or a VPN can change the addresses without a primary-interface
 * change. The local read is cheap (no network request), so it runs often
 * enough that a VPN toggle shows within seconds.
 */
const LOCAL_REFRESH_MS = 10_000;
/** The public lookup is a request to an outside service; ask rarely. */
const PUBLIC_STALE_MS = 10 * 60_000;

/**
 * The primary interface's addresses, read locally, and the public address
 * from an outside service (D-093). The local read is keyed by the primary
 * interface and refreshes every 10 s; the public one by the interface, the
 * local address it read and the interface internet traffic leaves through,
 * so joining another network or a VPN asks again. The
 * public lookup waits for the local read, so a remote host (which has none
 * here) never shows this Mac's address. It runs only while the Network page
 * or the popover's Network card is shown (a hidden window pauses both
 * intervals), at most every 10 minutes per window, and is not retried on
 * failure. Showing the window re-reads the local addresses at once, so a
 * network changed while the popover was closed is caught on open.
 */
export function useAddresses(primary: string | null) {
  const transport = useTransport();
  const hostId = useHostId();
  const local = useQuery({
    queryKey: hostKeys.networkAddresses(hostId, primary),
    queryFn: () => unwrap(transport.getNetworkAddresses(hostId)),
    enabled: primary !== null,
    refetchInterval: LOCAL_REFRESH_MS,
    refetchOnWindowFocus: "always",
  });
  const lan = local.data?.ipv4[0] ?? local.data?.ipv6[0] ?? null;
  const egress = local.data?.egress ?? null;
  const publicIp = useQuery({
    queryKey: appKeys.publicIp(primary, lan, egress),
    queryFn: () => unwrap(transport.getPublicIp()),
    enabled: primary !== null && lan !== null,
    staleTime: PUBLIC_STALE_MS,
    gcTime: PUBLIC_STALE_MS,
    refetchInterval: PUBLIC_STALE_MS,
    retry: false,
  });
  return { lan, publicIp };
}
