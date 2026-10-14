import type {
  CommandError,
  RemoteHostPreparation,
  RemoteSshHostTrust,
} from "@core/generated/bindings";
import { Button } from "~/components/ui/button";
import { SshSetupHelp } from "./ssh-setup-help";

export function AddMacTrust({
  hostKeyError,
  error,
  prepared,
  trust,
  endpoints,
  busy,
  inspect,
  accept,
}: {
  hostKeyError: boolean;
  error: CommandError | string | null;
  prepared: RemoteHostPreparation | null;
  trust: RemoteSshHostTrust | null;
  endpoints: { lan: string | null; tailscale: string | null };
  busy: boolean;
  inspect: (alias: string) => Promise<void>;
  accept: () => Promise<void>;
}) {
  return (
    <>
      {" "}
      {hostKeyError && error && !prepared && !trust && (
        <div className="flex gap-2">
          {endpoints.lan && (
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => inspect(endpoints.lan as string)}
            >
              Verify LAN host
            </Button>
          )}
          {endpoints.tailscale && (
            <Button
              size="sm"
              variant="outline"
              disabled={busy}
              onClick={() => inspect(endpoints.tailscale as string)}
            >
              Verify Tailscale host
            </Button>
          )}
        </div>
      )}
      {trust && (
        <div className="grid gap-2 border-border-subtle border-y py-3 text-[12px]">
          <p>
            Verify {trust.hostname}:{trust.port} with the owner of that Mac
            before trusting its SSH key. On the other Mac, run this in Terminal
            and compare each SHA256 fingerprint:
          </p>
          <p className="select-text break-all">
            {
              'for key in /etc/ssh/ssh_host_*_key.pub; do ssh-keygen -lf "$key"; done'
            }
          </p>
          {trust.fingerprints.map((fingerprint) => (
            <p key={fingerprint} className="select-text break-all">
              {fingerprint}
            </p>
          ))}
          <Button disabled={busy} onClick={accept}>
            Trust this host
          </Button>
        </div>
      )}
      {(!prepared || error) && !trust && (
        <SshSetupHelp
          lan={endpoints.lan}
          tailscale={endpoints.tailscale}
          busy={busy}
        />
      )}
    </>
  );
}
