import type { RemoteUpdateStatus } from "@core/generated/bindings";
import { hostKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { SettingsRow } from "~/components/settings-row";
import { Button } from "~/components/ui/button";
import { Switch } from "~/components/ui/switch";
import { useTransport } from "~/lib/transport-context";

export const COORDINATED_UPGRADE_GUIDE =
  "https://github.com/tryopendata/kelvo/blob/main/.github/releasing.md#coordinated-protocol-upgrades";

function message(status: RemoteUpdateStatus): string {
  if (!status.supported)
    return (
      status.message ??
      "Update Kelvo on this host once to enable automatic updates."
    );
  if (status.message) return status.message;
  switch (status.status) {
    case "available":
      return status.version
        ? `Kelvo ${status.version} is available on this host.`
        : "An update is available on this host.";
    case "up_to_date":
      return "The host is running the latest compatible release.";
    case "checking":
      return "Checking for a new release…";
    case "downloading":
      return "Downloading an update…";
    case "installing":
      return "Installing an update. Monitoring reconnects automatically.";
    case "updated":
    case "ready":
      return status.version
        ? `Updated to Kelvo ${status.version}.`
        : "Update installed.";
    case "failed":
      return "The last update failed. The host kept its previous version.";
    case "waiting":
    case "downloaded":
      return "An update is ready and waiting to install.";
    case "deferred":
    case "compatibility_blocked":
      return "Update deferred to keep existing connections compatible.";
    case "needs_authorization":
      return "Open Kelvo on this Mac to approve installation.";
    default:
      return status.check_updates && status.auto_install_updates
        ? "This host installs compatible releases even when this Mac is offline."
        : "Automatic installation is off on this host.";
  }
}

/** Mounted only while this host's settings are open; no SSH polling after closing. */
export function RemoteUpdateSettings({ hostId }: { hostId: string }) {
  const transport = useTransport();
  const cache = useQueryClient();
  const key = hostKeys.updates(hostId);
  const status = useQuery({
    queryKey: key,
    queryFn: () => unwrap(transport.getRemoteUpdateStatus(hostId)),
    retry: false,
    refetchInterval: 30000,
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const update = async (checkUpdates: boolean, autoInstallUpdates: boolean) => {
    setSaving(true);
    setError(null);
    try {
      const next = await unwrap(
        transport.setRemoteUpdatePolicy(
          hostId,
          checkUpdates,
          autoInstallUpdates
        )
      );
      cache.setQueryData(key, next);
    } catch (cause) {
      setError(String(cause));
    } finally {
      setSaving(false);
    }
  };
  const current = status.data;
  return (
    <div className="-mx-4">
      <SettingsRow
        label="Updates on this host"
        sub={
          <span role="status">
            {status.isPending
              ? "Reading update settings from the host…"
              : status.isError
                ? "Couldn’t read this host’s update settings."
                : current
                  ? message(current)
                  : "Update status unavailable."}
          </span>
        }
      >
        {current?.status === "compatibility_blocked" && (
          <Button
            size="sm"
            variant="outline"
            onClick={() =>
              void transport
                .openUrl(COORDINATED_UPGRADE_GUIDE)
                .catch((cause: unknown) => setError(String(cause)))
            }
          >
            Upgrade guide
          </Button>
        )}
        <Button
          size="sm"
          variant="outline"
          disabled={status.isFetching || saving}
          onClick={() => void status.refetch()}
        >
          Refresh
        </Button>
      </SettingsRow>
      {current?.supported && !status.isError && (
        <>
          <SettingsRow label="Check for updates automatically">
            <Switch
              aria-label="Check for host updates automatically"
              checked={current.check_updates}
              disabled={saving || status.isFetching}
              onCheckedChange={(on) =>
                void update(on, current.auto_install_updates)
              }
            />
          </SettingsRow>
          <SettingsRow label="Install updates automatically">
            <Switch
              aria-label="Install host updates automatically"
              checked={current.auto_install_updates}
              disabled={saving || status.isFetching || !current.check_updates}
              onCheckedChange={(on) => void update(current.check_updates, on)}
            />
          </SettingsRow>
        </>
      )}
      {(error || status.error) && (
        <p role="alert" className="text-[12px] text-destructive">
          {error ?? String(status.error)}
        </p>
      )}
    </div>
  );
}
