import type { UpdateStatus } from "@core/generated/bindings";
import { useEffect, useState } from "react";
import { SettingsRow } from "~/components/settings-row";
import { Button } from "~/components/ui/button";
import { useTransport } from "~/lib/transport-context";
import { version } from "../../../../../../package.json";

function updateMessage(status: UpdateStatus): string {
  switch (status.kind) {
    case "idle":
      return "Updates install only when you choose.";
    case "not_configured":
      return "Use a release build to check for updates.";
    case "checking":
      return "Checking GitHub Releases…";
    case "up_to_date":
      return "You’re running the latest version.";
    case "available":
      return `Kelvo ${status.version} is available. Installing restarts Kelvo; your history and settings are kept.`;
    case "downloading":
      return status.percent === null
        ? "Downloading update…"
        : `Downloading update… ${status.percent}%`;
    case "installing":
      return "Installing update…";
    case "ready":
      return "Update installed. Restarting Kelvo…";
    case "failed":
      return `Couldn’t update: ${status.message}`;
  }
}

export function UpdateRow() {
  const transport = useTransport();
  const [status, setStatus] = useState<UpdateStatus>({ kind: "idle" });
  const [pending, setPending] = useState(false);
  useEffect(() => {
    let active = true;
    let receivedEvent = false;
    const stop = transport.onUpdateStatusChanged((next) => {
      receivedEvent = true;
      if (active) setStatus(next);
    });
    transport
      .getUpdateStatus()
      .then((next) => {
        if (active && !receivedEvent) setStatus(next);
      })
      .catch((error: unknown) => {
        if (active && !receivedEvent)
          setStatus({ kind: "failed", message: String(error) });
      });
    return () => {
      active = false;
      stop();
    };
  }, [transport]);

  const act = async () => {
    setPending(true);
    try {
      const result =
        status.kind === "available"
          ? await transport.installUpdate(status.version)
          : await transport.checkForUpdates();
      if (result.status === "error") {
        setStatus({
          kind: "failed",
          message:
            "message" in result.error
              ? result.error.message
              : "Please try again.",
        });
      } else if (result.data) {
        setStatus(result.data);
      }
    } catch (error) {
      setStatus({ kind: "failed", message: String(error) });
    } finally {
      setPending(false);
    }
  };
  const busy =
    pending ||
    ["checking", "downloading", "installing", "ready"].includes(status.kind);
  return (
    <SettingsRow
      label="Version"
      sub={
        <>
          <span role="status">{updateMessage(status)}</span>
          {status.kind === "available" && status.notes && (
            <details className="mt-2">
              <summary className="cursor-pointer">Release notes</summary>
              <span className="mt-2 block max-h-40 overflow-auto whitespace-pre-wrap">
                {status.notes}
              </span>
            </details>
          )}
        </>
      }
    >
      <span className="figures text-[12px] text-fg-subtle">{version}</span>
      <Button
        variant="outline"
        size="sm"
        disabled={busy || status.kind === "not_configured"}
        onClick={act}
      >
        {status.kind === "available"
          ? "Install and restart"
          : busy
            ? "Please wait…"
            : "Check now"}
      </Button>
    </SettingsRow>
  );
}
