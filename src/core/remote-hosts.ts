import type { RemoteHostRecord } from "@core/generated/bindings";

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
