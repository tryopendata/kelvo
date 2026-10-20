import type {
  LinuxPreparation,
  RemoteHostRecord,
} from "@core/generated/bindings";

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

/** What the probe found on a Linux host: "Linux 6.17 · aarch64 · systemd 256". */
export function linuxSystemLine(linux: LinuxPreparation): string {
  const kernel = linux.kernel?.match(/^\d+\.\d+/)?.[0];
  return [
    kernel ? `Linux ${kernel}` : "Linux",
    linux.arch,
    linux.systemd_version === null ? null : `systemd ${linux.systemd_version}`,
  ]
    .filter((part) => part)
    .join(" · ");
}
