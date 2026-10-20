import type {
  RemoteHostPreparation,
  RemoteSshHostTrust,
} from "@core/generated/bindings";
import { Button } from "~/components/ui/button";
import { DialogFooter } from "~/components/ui/dialog";
import type { SetupPhase } from "./use-add-host-connection";

/** The action a prepared host offers, and what it does. */
function connectAction(prepared: RemoteHostPreparation) {
  if (prepared.linux?.duplicate_of) return "Reset identity and connect";
  if (prepared.installed && prepared.compatible) return "Connect";
  return prepared.installed ? "Update and connect" : "Install and connect";
}

export function AddHostFooter({
  phase,
  busy,
  cancel,
  trust,
  prepared,
  add,
  resetAndAdd,
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
  resetAndAdd: () => Promise<void>;
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
        <Button
          disabled={busy}
          onClick={prepared.linux?.duplicate_of ? resetAndAdd : add}
        >
          {connectAction(prepared)}
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
