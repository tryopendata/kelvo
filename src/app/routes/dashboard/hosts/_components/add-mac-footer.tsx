import type {
  RemoteHostPreparation,
  RemoteSshHostTrust,
} from "@core/generated/bindings";
import { Button } from "~/components/ui/button";
import { DialogFooter } from "~/components/ui/dialog";
import type { SetupPhase } from "./use-add-mac-connection";
export function AddMacFooter({
  phase,
  busy,
  cancel,
  trust,
  prepared,
  add,
  endpointError,
  endpoints,
  prepare,
}: {
  phase: SetupPhase;
  busy: boolean;
  cancel: () => Promise<void>;
  trust: RemoteSshHostTrust | null;
  prepared: RemoteHostPreparation | null;
  add: () => Promise<void>;
  endpointError: string | null;
  endpoints: { lan: string | null; tailscale: string | null };
  prepare: () => Promise<void>;
}) {
  return (
    <DialogFooter>
      <Button
        variant="outline"
        disabled={busy && phase !== "adding"}
        onClick={cancel}
      >
        {phase === "cancelling" ? "Cancelling…" : "Cancel"}
      </Button>
      {trust ? null : prepared ? (
        <Button disabled={busy} onClick={add}>
          {prepared.installed && prepared.compatible
            ? "Connect"
            : prepared.installed
              ? "Update and connect"
              : "Install and connect"}
        </Button>
      ) : (
        <Button
          disabled={
            busy || !!endpointError || (!endpoints.lan && !endpoints.tailscale)
          }
          onClick={prepare}
        >
          Check connection
        </Button>
      )}
    </DialogFooter>
  );
}
