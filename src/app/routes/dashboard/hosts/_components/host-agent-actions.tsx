import type { CommandError, RemoteHostRecord } from "@core/generated/bindings";
import { hostKeys } from "@core/query-keys";
import { remoteErrorText } from "@core/remote-errors";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "~/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from "~/components/ui/dialog";
import { useTransport } from "~/lib/transport-context";
import { RemoteSetupError } from "./remote-setup-error";

/** Re-pairs a host that answers with a new identity, after saying what happens to the old one. */
function RepairDialog({
  remote,
  close,
}: {
  remote: RemoteHostRecord;
  close: () => void;
}) {
  const transport = useTransport();
  const cache = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CommandError | string | null>(null);
  const repair = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await transport.repairRemoteHost(remote.config.id);
      if (result.status === "error") setError(result.error);
      else {
        await cache.invalidateQueries({ queryKey: hostKeys.all });
        close();
      }
    } catch (e) {
      console.error("[remote hosts] re-pair failed", {
        hostId: remote.config.id,
        error: e,
      });
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
    >
      <DialogContent showCloseButton={!busy}>
        <DialogTitle>Re-pair {remote.config.name}?</DialogTitle>
        <DialogDescription>
          The host at this address now has a different identity, as after a
          reinstall or a replaced machine. Kelvo adds it as a new host and keeps{" "}
          {remote.config.name}’s mirrored history here as an offline host.
        </DialogDescription>
        {error && <RemoteSetupError error={error} />}
        <DialogFooter>
          <Button variant="outline" disabled={busy} onClick={close}>
            Cancel
          </Button>
          <Button disabled={busy} onClick={repair}>
            {busy ? "Re-pairing…" : "Re-pair as a new host"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** The host-list actions a host's status offers: an agent update, or re-pairing. */
export function HostAgentActions({
  remote,
  onError,
}: {
  remote: RemoteHostRecord;
  onError: (message: string | null) => void;
}) {
  const transport = useTransport();
  const cache = useQueryClient();
  const [updating, setUpdating] = useState(false);
  const [repairing, setRepairing] = useState(false);
  const update = async () => {
    setUpdating(true);
    onError(null);
    try {
      const result = await transport.updateRemoteAgent(remote.config.id);
      if (result.status === "error") onError(remoteErrorText(result.error));
      await cache.invalidateQueries({ queryKey: hostKeys.remote() });
    } catch (e) {
      console.error("[remote hosts] agent update failed", {
        hostId: remote.config.id,
        error: e,
      });
      onError(String(e));
    } finally {
      setUpdating(false);
    }
  };
  return (
    <>
      {remote.status.identity_changed !== null && (
        <Button size="sm" variant="outline" onClick={() => setRepairing(true)}>
          Re-pair…
        </Button>
      )}
      {remote.status.update_available && (
        <Button
          size="sm"
          variant="outline"
          disabled={updating}
          onClick={update}
        >
          {updating ? "Updating agent…" : "Update agent"}
        </Button>
      )}
      {repairing && (
        <RepairDialog remote={remote} close={() => setRepairing(false)} />
      )}
    </>
  );
}
