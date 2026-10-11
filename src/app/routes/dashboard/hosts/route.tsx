import { formatBytes } from "@core/format";
import type { RemoteHostRecord } from "@core/generated/bindings";
import { hostKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useNavigate } from "react-router";
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
import { AddMacDialog, HOST_INPUT } from "./_components/add-mac-dialog";

export function connectionLabel(remote: RemoteHostRecord): string {
  const { status } = remote;
  if (status.state === "connected")
    return `${status.route === "lan" ? "LAN" : "Tailscale"}${status.sync === "catching_up" ? " · catching up history" : status.sync === "unavailable" ? " · history unavailable" : " · connected"}`;
  return (
    {
      connecting: "Connecting…",
      offline: "Offline",
      disabled: "Connection disabled",
      error: "Connection failed",
    }[status.state] ?? status.state
  );
}

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
  const [deleteMirror, setDeleteMirror] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const run = async (remove: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const result = remove
        ? await transport.removeRemoteHost(config.id, deleteMirror)
        : await transport.updateRemoteHost(config);
      if (result.status === "error")
        setError(
          "message" in result.error ? result.error.message : result.error.kind
        );
      else {
        await cache.invalidateQueries({ queryKey: hostKeys.all });
        close();
      }
    } catch (e) {
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
            ? "This disconnects the Mac. Kelvo on that Mac keeps running."
            : "Connection and history stored on this Mac."}
        </DialogDescription>
        {removing ? (
          <label className="flex items-center gap-2 text-[13px]">
            <input
              type="checkbox"
              checked={deleteMirror}
              onChange={(e) => setDeleteMirror(e.target.checked)}
            />
            Delete mirrored history from this Mac
          </label>
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
                max="100000"
                value={config.mirror_budget_mb}
                onChange={(e) =>
                  setConfig({
                    ...config,
                    mirror_budget_mb: Number(e.target.value),
                  })
                }
              />
            </label>
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
        {error && (
          <p role="alert" className="text-[13px] text-destructive">
            {error}
          </p>
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
              Remove Mac…
            </Button>
          )}
          <Button
            disabled={
              busy ||
              !config.name.trim() ||
              !(config.lan?.trim() || config.tailscale?.trim()) ||
              config.mirror_budget_mb < 10
            }
            onClick={() => run(removing)}
          >
            {busy ? "Saving…" : removing ? "Remove Mac" : "Save"}
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
    navigate("/dashboard/overview");
  };
  const retry = async (id: string) => {
    setError(null);
    try {
      const result = await transport.retryRemoteHost(id);
      if (result.status === "error")
        setError(
          "message" in result.error ? result.error.message : result.error.kind
        );
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
        actions={<Button onClick={() => setAdding(true)}>Add Mac</Button>}
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
              {remote.status.error && (
                <p className="mt-1 text-[12px] text-destructive">
                  {remote.status.error}
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
              {remote.status.state !== "connected" && remote.config.enabled && (
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
          Loading saved Macs…
        </p>
      ) : (
        remotes.data?.length === 0 && (
          <p className="text-[13px] text-muted-foreground">
            Add a Mac to view its live metrics and keep a copy of its history
            here.
          </p>
        )
      )}
      {(error || remotes.error) && (
        <p role="alert" className="text-[13px] text-destructive">
          {error ?? String(remotes.error)}
        </p>
      )}
      {adding && <AddMacDialog open onOpenChange={setAdding} />}
      {editing && (
        <HostSettings remote={editing} close={() => setEditing(null)} />
      )}
    </div>
  );
}
