import type {
  CommandError,
  RemoteHostPreparation,
  RemoteSshHostTrust,
} from "@core/generated/bindings";
import { isHostKeyError, sshEndpointError } from "@core/remote-errors";
import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "~/components/ui/dialog";
import { useTransport } from "~/lib/transport-context";
import { AddMacFields } from "./add-mac-fields";
import { AddMacFooter } from "./add-mac-footer";
import { AddMacStatus } from "./add-mac-status";
import { AddMacTrust } from "./add-mac-trust";
import { type SetupPhase, useAddMacConnection } from "./use-add-mac-connection";

export { HOST_INPUT } from "./host-input";

export function AddMacDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const transport = useTransport();
  const [name, setName] = useState("");
  const [lan, setLan] = useState("");
  const [tailscale, setTailscale] = useState("");
  const [showMenuBar, setShowMenuBar] = useState(true);
  const [prepared, setPrepared] = useState<RemoteHostPreparation | null>(null);
  const [trust, setTrust] = useState<RemoteSshHostTrust | null>(null);
  const [phase, setPhase] = useState<SetupPhase>("idle");
  const [hostKeyError, setHostKeyError] = useState(false);
  const [error, setError] = useState<CommandError | string | null>(null);
  const busy = phase !== "idle";
  const endpoints = {
    lan: lan.trim() || null,
    tailscale: tailscale.trim() || null,
  };
  const endpointError = sshEndpointError(lan) ?? sshEndpointError(tailscale);
  const prepare = async () => {
    if (endpointError) return;
    setHostKeyError(false);
    setPrepared(null);
    setTrust(null);
    setPhase("checking");
    setError(null);
    try {
      const result = await transport.prepareRemoteHost(
        endpoints.lan,
        endpoints.tailscale
      );
      if (result.status === "error") {
        setHostKeyError(isHostKeyError(result.error));
        setError(result.error);
      } else {
        setPrepared(result.data);
        if (!name.trim()) setName(result.data.name);
      }
    } catch (e) {
      console.error("[remote setup] operation failed", {
        phase: "prepare",
        lan,
        tailscale,
        error: e,
      });
      setError(String(e));
    } finally {
      setPhase("idle");
    }
  };
  const inspect = async (alias: string) => {
    setPhase("verifying");
    setError(null);
    setTrust(null);
    try {
      const result = await transport.inspectRemoteSshHost(alias);
      if (result.status === "error") setError(result.error);
      else {
        setError(null);
        setTrust(result.data);
      }
    } catch (e) {
      console.error("[remote setup] inspect failed", {
        lan,
        tailscale,
        error: e,
      });
      setError(String(e));
    } finally {
      setPhase("idle");
    }
  };
  const accept = async () => {
    if (!trust) return;
    setPhase("verifying");
    try {
      const result = await transport.acceptRemoteSshHost(trust.token);
      if (result.status === "error") {
        setError(result.error);
        setTrust(null);
        setHostKeyError(true);
      } else {
        setTrust(null);
        await prepare();
      }
    } catch (e) {
      console.error("[remote setup] accept failed", {
        lan,
        tailscale,
        error: e,
      });
      setTrust(null);
      setHostKeyError(true);
      setError(String(e));
    } finally {
      setPhase("idle");
    }
  };
  const { add, cancel } = useAddMacConnection({
    prepared,
    name,
    endpoints,
    showMenuBar,
    phase,
    setPhase,
    setError,
    setPrepared,
    setHostKeyError,
    onOpenChange,
    lan,
    tailscale,
  });
  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!busy) onOpenChange(next);
      }}
    >
      <DialogContent
        showCloseButton={!busy}
        className="max-h-[calc(100svh-2rem)] overflow-y-auto"
      >
        <DialogTitle>Add Mac</DialogTitle>
        <DialogDescription>
          Connect through SSH. Kelvo prefers LAN and falls back to Tailscale
          when both are configured. Kelvo on that Mac will start at login and
          share its metrics with this Mac.
        </DialogDescription>
        <AddMacFields
          busy={busy}
          name={name}
          setName={setName}
          lan={lan}
          setLan={setLan}
          tailscale={tailscale}
          setTailscale={setTailscale}
          showMenuBar={showMenuBar}
          setShowMenuBar={setShowMenuBar}
          setPrepared={setPrepared}
          setTrust={setTrust}
          setHostKeyError={setHostKeyError}
          setError={setError}
        />
        <AddMacStatus
          endpointError={endpointError}
          prepared={prepared}
          error={error}
          busy={busy}
          phase={phase}
        />
        <AddMacTrust
          hostKeyError={hostKeyError}
          error={error}
          prepared={prepared}
          trust={trust}
          endpoints={endpoints}
          busy={busy}
          inspect={inspect}
          accept={accept}
        />
        <AddMacFooter
          phase={phase}
          busy={busy}
          cancel={cancel}
          trust={trust}
          prepared={prepared}
          add={add}
          endpointError={endpointError}
          endpoints={endpoints}
          prepare={prepare}
        />
      </DialogContent>
    </Dialog>
  );
}
