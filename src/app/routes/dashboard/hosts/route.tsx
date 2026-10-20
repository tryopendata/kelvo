import { formatBytes } from "@core/format";
import type { CommandError, RemoteHostRecord } from "@core/generated/bindings";
import { hostKeys } from "@core/query-keys";
import { remoteErrorText, sshEndpointError } from "@core/remote-errors";
import { connectionLabel } from "@core/remote-hosts";
import { unwrap } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate } from "react-router";
import { toast } from "sonner";
import { PageHeader } from "~/components/page-header";
import { Button } from "~/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "~/components/ui/dialog";
import { Switch } from "~/components/ui/switch";
import { useRemoteHosts } from "~/hooks/use-remote-hosts";
import { useHostSelection } from "~/lib/host-selection";
import { useTransport } from "~/lib/transport-context";
import { AddHostDialog, HOST_INPUT } from "./_components/add-host-dialog";
import { HostAgentActions } from "./_components/host-agent-actions";
import { RemoteSetupError } from "./_components/remote-setup-error";
import {
  type RemoveChoices,
  RemoveHostOptions,
} from "./_components/remove-host-options";
import { SshSetupHelp } from "./_components/ssh-setup-help";

// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Controller and presentation split tracked in architecture refactor package H.
function HostSettings({
  remote,
  close,
}: {
  remote: RemoteHostRecord;
  close: () => void;
}) {
  const transport = useTransport();
  const cache = useQueryClient();
  const [config, setConfig] = useState(remote.config);
  const size = useQuery({
    queryKey: hostKeys.historySize(remote.config.id),
    queryFn: () => unwrap(transport.historySize(remote.config.id)),
    refetchInterval: 60000,
  });
  const linux = remote.os === "linux";
  const [choices, setChoices] = useState<RemoveChoices>({
    stopSharing: true,
    uninstallAgent: true,
    purgeAgent: false,
    deleteMirror: false,
  });
  const [removing, setRemoving] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<CommandError | string | null>(null);
  const endpointError =
    sshEndpointError(config.lan ?? "") ??
    sshEndpointError(config.tailscale ?? "");
  const validBudget =
    Number.isInteger(config.mirror_budget_mb) &&
    config.mirror_budget_mb >= 10 &&
    config.mirror_budget_mb <= 10000;
  const run = async (remove: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const result = remove
        ? await transport.removeRemoteHost(
            config.id,
            choices.deleteMirror,
            !linux && choices.stopSharing,
            linux && choices.uninstallAgent,
            linux && choices.uninstallAgent && choices.purgeAgent
          )
        : await transport.updateRemoteHost(config);
      if (result.status === "error") setError(result.error);
      else {
        if (remove && typeof result.data === "string") toast.error(result.data);
        await cache.invalidateQueries({ queryKey: hostKeys.all });
        close();
      }
    } catch (e) {
      console.error("[remote settings] update failed", {
        hostId: config.id,
        remove,
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
      <DialogContent
        showCloseButton={!busy}
        className="max-h-[calc(100svh-2rem)] overflow-y-auto"
      >
        <DialogTitle>
          {removing ? `Remove ${config.name}?` : config.name}
        </DialogTitle>
        <DialogDescription>
          {removing
            ? linux
              ? "This disconnects the server. Unless you uninstall it, the agent keeps running and recording history there."
              : "This disconnects the Mac. Kelvo on that Mac keeps running and its start-at-login setting stays the same."
            : "Connection and history stored on this Mac."}
        </DialogDescription>
        {removing ? (
          <RemoveHostOptions
            linux={linux}
            choices={choices}
            setChoices={setChoices}
            busy={busy}
          />
        ) : (
          <fieldset disabled={busy} className="grid gap-3">
            <label className="grid gap-1 text-[12px]">
              Name
              <input
                className={HOST_INPUT}
                value={config.name}
                onChange={(e) => setConfig({ ...config, name: e.target.value })}
              />
            </label>
            <label className="grid gap-1 text-[12px]">
              LAN SSH alias
              <input
                className={HOST_INPUT}
                value={config.lan ?? ""}
                onChange={(e) =>
                  setConfig({ ...config, lan: e.target.value || null })
                }
              />
            </label>
            <label className="grid gap-1 text-[12px]">
              Tailscale SSH alias
              <input
                className={HOST_INPUT}
                value={config.tailscale ?? ""}
                onChange={(e) =>
                  setConfig({ ...config, tailscale: e.target.value || null })
                }
              />
            </label>
            <p className="text-[12px] text-muted-foreground">
              Mirrored history:{" "}
              {size.data === undefined
                ? size.isError
                  ? "unavailable"
                  : "loading…"
                : formatBytes(size.data)}
            </p>
            <label className="grid gap-1 text-[12px]">
              History limit (MB)
              <input
                className={HOST_INPUT}
                type="number"
                min="10"
                max="10000"
                value={
                  Number.isNaN(config.mirror_budget_mb)
                    ? ""
                    : config.mirror_budget_mb
                }
                onChange={(e) =>
                  setConfig({
                    ...config,
                    mirror_budget_mb:
                      e.target.value === ""
                        ? Number.NaN
                        : Number(e.target.value),
                  })
                }
              />
            </label>
            {!validBudget && (
              <p role="alert" className="text-[13px] text-destructive">
                Enter a whole number from 10 to 10,000 MB.
              </p>
            )}
            <div className="flex justify-between text-[13px]">
              Connect automatically
              <Switch
                aria-label="Connect automatically"
                checked={config.enabled}
                onCheckedChange={(enabled) => setConfig({ ...config, enabled })}
              />
            </div>
          </fieldset>
        )}
        {!removing && endpointError && (
          <p role="alert" className="text-[12px] text-destructive">
            {endpointError}
          </p>
        )}
        {error && <RemoteSetupError error={error} />}
        {!removing && (
          <SshSetupHelp
            saved
            lan={config.lan}
            tailscale={config.tailscale}
            busy={busy}
          />
        )}
        <div className="flex justify-between gap-2">
          {removing ? (
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => setRemoving(false)}
            >
              Back
            </Button>
          ) : (
            <Button
              variant="ghost"
              disabled={busy}
              onClick={() => setRemoving(true)}
            >
              Remove host…
            </Button>
          )}
          <Button
            disabled={
              busy ||
              !config.name.trim() ||
              !(config.lan?.trim() || config.tailscale?.trim()) ||
              (!removing && (!validBudget || !!endpointError))
            }
            onClick={() => run(removing)}
          >
            {busy ? "Saving…" : removing ? "Remove host" : "Save"}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

export default function HostsRoute() {
  const selection = useHostSelection();
  const transport = useTransport();
  const cache = useQueryClient();
  const navigate = useNavigate();
  const remotes = useRemoteHosts();
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<RemoteHostRecord | null>(null);
  const [error, setError] = useState<string | null>(null);
  const show = (id: string) => {
    selection?.select(id);
    void navigate("/dashboard/overview");
  };
  const retry = async (id: string) => {
    setError(null);
    try {
      const result = await transport.retryRemoteHost(id);
      if (result.status === "error") setError(remoteErrorText(result.error));
      await cache.invalidateQueries({ queryKey: hostKeys.remote() });
    } catch (e) {
      setError(String(e));
    }
  };
  const local = selection?.hosts.find((host) => host.is_local);
  return (
    <div className="flex flex-col gap-5">
      <PageHeader
        title="Hosts"
        actions={<Button onClick={() => setAdding(true)}>Add host</Button>}
      />
      <div className="divide-y divide-border-subtle border-border-subtle border-y">
        {local && (
          <div className="flex items-center justify-between gap-4 py-4">
            <div>
              <h2 className="font-[590] text-[14px]">This Mac</h2>
              <p className="text-[12px] text-muted-foreground">
                {local.display_name} · {local.info.chip ?? local.info.model}
              </p>
            </div>
            <Button variant="outline" size="sm" onClick={() => show(local.id)}>
              View metrics
            </Button>
          </div>
        )}
        {remotes.data?.map((remote) => (
          <div
            key={remote.config.id}
            className="flex flex-wrap items-center justify-between gap-4 py-4"
          >
            <div className="min-w-0 flex-1">
              <h2 className="font-[590] text-[14px]">{remote.config.name}</h2>
              <p className="text-[12px] text-muted-foreground">
                {connectionLabel(remote)}
              </p>
              {remote.status.last_seen_ms !== null &&
                remote.status.state !== "connected" && (
                  <p className="text-[12px] text-muted-foreground">
                    Last seen{" "}
                    {new Date(remote.status.last_seen_ms).toLocaleString()}
                  </p>
                )}
              {remote.status.identity_changed !== null ? (
                <p className="mt-1 text-[12px] text-destructive">
                  This host now answers with a different identity.
                </p>
              ) : (
                remote.status.error && (
                  <p className="mt-1 text-[12px] text-destructive">
                    {remote.status.error}
                  </p>
                )
              )}
              {remote.status.update_available && (
                <p className="mt-1 text-[12px] text-muted-foreground">
                  Agent {remote.status.agent_version} · this Kelvo includes a
                  newer one
                </p>
              )}
            </div>
            <dl className="figures flex gap-5 text-[12px]">
              <div>
                <dt className="text-muted-foreground">CPU</dt>
                <dd className="min-w-10">
                  {remote.status.state === "connected" &&
                  remote.status.cpu_pct != null
                    ? `${Math.round(remote.status.cpu_pct)}%`
                    : "—"}
                </dd>
              </div>
              <div>
                <dt className="text-muted-foreground">Memory</dt>
                <dd className="min-w-10">
                  {remote.status.state === "connected" &&
                  remote.status.memory_pct != null
                    ? `${Math.round(remote.status.memory_pct)}%`
                    : "—"}
                </dd>
              </div>
            </dl>
            <div className="flex gap-2">
              <HostAgentActions remote={remote} onError={setError} />
              {remote.status.state !== "connected" &&
                remote.config.enabled &&
                remote.status.identity_changed === null && (
                  <Button
                    size="sm"
                    variant="ghost"
                    onClick={() => retry(remote.config.id)}
                  >
                    Retry
                  </Button>
                )}
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setEditing(remote)}
              >
                Settings
              </Button>
              <Button
                variant="outline"
                size="sm"
                onClick={() => show(remote.config.id)}
              >
                View metrics
              </Button>
            </div>
          </div>
        ))}
      </div>
      {remotes.isPending ? (
        <p role="status" className="text-[13px] text-muted-foreground">
          Loading saved hosts…
        </p>
      ) : (
        remotes.data?.length === 0 && (
          <p className="text-[13px] text-muted-foreground">
            Add a Linux server or another Mac over SSH to view its live metrics
            and keep a copy of its history here.
          </p>
        )
      )}
      {(error || remotes.error) && (
        <p role="alert" className="text-[13px] text-destructive">
          {error ?? String(remotes.error)}
        </p>
      )}
      {adding && <AddHostDialog open onOpenChange={setAdding} />}
      {editing && (
        <HostSettings remote={editing} close={() => setEditing(null)} />
      )}
    </div>
  );
}
