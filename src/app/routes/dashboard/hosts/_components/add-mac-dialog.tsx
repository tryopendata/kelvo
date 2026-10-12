import type {
  CommandError,
  RemoteHostConfig,
  RemoteHostPreparation,
  RemoteSshHostTrust,
} from "@core/generated/bindings";
import { hostKeys } from "@core/query-keys";
import { isHostKeyError, sshEndpointError } from "@core/remote-errors";
import { useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { Button } from "~/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogTitle,
} from "~/components/ui/dialog";
import { Switch } from "~/components/ui/switch";
import { useTransport } from "~/lib/transport-context";
import { RemoteSetupError } from "./remote-setup-error";
import { SshSetupHelp } from "./ssh-setup-help";

export const HOST_INPUT =
  "h-8 w-full rounded-control border border-input bg-background px-2 text-[13px] outline-none focus-visible:ring-2 focus-visible:ring-ring";

export function AddMacDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const transport = useTransport();
  const cache = useQueryClient();
  const [name, setName] = useState("");
  const [lan, setLan] = useState("");
  const [tailscale, setTailscale] = useState("");
  const [showMenuBar, setShowMenuBar] = useState(true);
  const [prepared, setPrepared] = useState<RemoteHostPreparation | null>(null);
  const [trust, setTrust] = useState<RemoteSshHostTrust | null>(null);
  const [phase, setPhase] = useState<
    "idle" | "checking" | "adding" | "verifying" | "cancelling"
  >("idle");
  const setupId = useRef<string | null>(null);
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
  const add = async () => {
    if (!prepared) return;
    const attempt = crypto.randomUUID();
    setupId.current = attempt;
    setPhase("adding");
    setError(null);
    const config: RemoteHostConfig = {
      id: prepared.host_id ?? "",
      name: name.trim() || prepared.name,
      ...endpoints,
      enabled: true,
      mirror_budget_mb: 100,
    };
    try {
      const result = await transport.addRemoteHost(
        config,
        !prepared.installed || !prepared.compatible,
        showMenuBar,
        attempt
      );
      if (result.status === "error") {
        setError(result.error);
        if (isHostKeyError(result.error)) {
          setPrepared(null);
          setHostKeyError(true);
        }
      } else {
        await cache.invalidateQueries({ queryKey: hostKeys.all });
        onOpenChange(false);
      }
    } catch (e) {
      console.error("[remote setup] add failed", { lan, tailscale, error: e });
      setError(String(e));
    } finally {
      setupId.current = null;
      setPhase("idle");
    }
  };
  const cancel = async () => {
    if (phase === "idle") {
      onOpenChange(false);
      return;
    }
    const attempt = setupId.current;
    if (phase !== "adding" || attempt === null) return;
    setPhase("cancelling");
    try {
      const result = await transport.cancelRemoteSetup(attempt);
      if (setupId.current === attempt && result.status === "error")
        setError(result.error);
    } catch (e) {
      console.error("[remote setup] cancel failed", {
        lan,
        tailscale,
        error: e,
      });
      if (setupId.current === attempt) setError(String(e));
    }
    // The original add response owns completion: its rollback must finish first.
  };
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
        <fieldset disabled={busy} className="grid gap-3">
          <label className="grid gap-1 text-[12px]">
            Name
            <input
              className={HOST_INPUT}
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Studio Mac"
            />
          </label>
          <label className="grid gap-1 text-[12px]">
            LAN SSH alias
            <input
              className={HOST_INPUT}
              value={lan}
              onChange={(e) => {
                setLan(e.target.value);
                setPrepared(null);
                setTrust(null);
                setHostKeyError(false);
                setError(null);
              }}
              placeholder="studio or username@studio.local"
              autoCapitalize="none"
              spellCheck={false}
            />
          </label>
          <label className="grid gap-1 text-[12px]">
            Tailscale SSH alias
            <input
              className={HOST_INPUT}
              value={tailscale}
              onChange={(e) => {
                setTailscale(e.target.value);
                setPrepared(null);
                setTrust(null);
                setHostKeyError(false);
                setError(null);
              }}
              placeholder="studio-tailnet"
              autoCapitalize="none"
              spellCheck={false}
            />
          </label>
          <p className="text-[12px] text-muted-foreground">
            Enter an alias from ~/.ssh/config or user@address, without “ssh”.
            Use the account signed in on the other Mac. Only one address is
            needed.
          </p>
          <div className="flex items-center justify-between gap-3 text-[12px]">
            <span>Show menu-bar icon on the other Mac</span>
            <Switch
              aria-label="Show menu-bar icon on remote Mac"
              checked={showMenuBar}
              onCheckedChange={setShowMenuBar}
            />
          </div>
        </fieldset>
        {endpointError && (
          <p role="alert" className="text-[12px] text-destructive">
            {endpointError}
          </p>
        )}
        {prepared && !error && (
          <p className="text-[13px] text-fg-subtle">
            {prepared.message ??
              (prepared.installed
                ? prepared.compatible
                  ? "Kelvo is ready to connect."
                  : "Kelvo needs an update. The installed app will be replaced after verification."
                : "Kelvo will be installed in your Applications folder on the other Mac.")}
          </p>
        )}
        {busy && (
          <p role="status" className="text-[13px] text-fg-subtle">
            {phase === "cancelling"
              ? "Cancelling… waiting for setup cleanup."
              : phase === "verifying"
                ? "Verifying SSH host…"
                : phase === "checking"
                  ? "Checking SSH access and Kelvo…"
                  : "Setting up Kelvo and connecting…"}
          </p>
        )}
        {error && <RemoteSetupError error={error} />}
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
              before trusting its SSH key. On the other Mac, run this in
              Terminal and compare each SHA256 fingerprint:
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
                busy ||
                !!endpointError ||
                (!endpoints.lan && !endpoints.tailscale)
              }
              onClick={prepare}
            >
              Check connection
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
