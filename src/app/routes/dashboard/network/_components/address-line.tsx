import { CopyValue } from "~/components/copy-value";
import { useAddresses } from "../_hooks/use-addresses";

/**
 * The header's addresses (D-093): the primary interface's IPv4 (its IPv6
 * when it has no IPv4) and the public address. Each copies on click.
 * Nothing shows until the local address is read.
 */
export function AddressLine({ primary }: { primary: string | null }) {
  const { lan, publicIp } = useAddresses(primary);
  // No local address: no network, or a host this Mac can't read addresses for.
  if (primary === null || lan === null) return null;
  return (
    <>
      {" · "}
      <CopyValue value={lan} label="local IP address" />
      {" · Public "}
      {publicIp.data ? (
        <CopyValue value={publicIp.data} label="public IP address" />
      ) : (
        <span className="text-muted-foreground">
          {publicIp.isError ? "unavailable" : "…"}
        </span>
      )}
    </>
  );
}
