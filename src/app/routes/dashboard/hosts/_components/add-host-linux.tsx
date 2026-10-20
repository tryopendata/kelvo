import { formatBytes } from "@core/format";
import type {
  LinuxPreparation,
  RemoteHostPreparation,
} from "@core/generated/bindings";
import { linuxSystemLine } from "@core/remote-hosts";
import { Command } from "./ssh-setup-help";

/** What setup will do on a Linux host, in the Mac status line's place (D-105). */
function plan(prepared: RemoteHostPreparation, linux: LinuxPreparation) {
  if (prepared.installed && prepared.compatible)
    return `Kelvo agent ${linux.existing_version} is installed and ready to connect.`;
  if (prepared.installed)
    return `Kelvo agent ${linux.existing_version ?? "(unknown version)"} is installed. It will be updated to this build’s version; its history and identity are kept.`;
  const service =
    linux.layout === "system"
      ? "a system service that runs as root and starts at boot"
      : `a user service for ${linux.user}`;
  return `Kelvo will install its agent at ${linux.binary_path} as ${service}.`;
}

export function AddHostLinux({
  prepared,
  linux,
}: {
  prepared: RemoteHostPreparation;
  linux: LinuxPreparation;
}) {
  const free =
    linux.free_bytes === null ? "" : ` (${formatBytes(linux.free_bytes)} free)`;
  return (
    <div className="grid gap-2 text-[13px] text-fg-subtle">
      <p>
        {linux.distro ?? "Linux"}
        <span className="text-muted-foreground">
          {" "}
          · {linuxSystemLine(linux)}
        </span>
      </p>
      <p>{plan(prepared, linux)}</p>
      {!prepared.installed && (
        <p>
          It keeps up to {linux.budget_mb} MB of history in {linux.data_dir}
          {free}.
        </p>
      )}
      {linux.duplicate_of !== null && (
        <p role="alert" className="text-destructive">
          This server’s agent has the same identity as {linux.duplicate_of},
          which is already in your hosts. If it is a copy of that server, give
          it a new identity to add it separately; {linux.duplicate_of} keeps its
          history.
        </p>
      )}
      {!linux.linger_ok && (
        <div className="grid gap-1 text-[12px]">
          <p>
            Lingering is off for {linux.user}, so the agent stops when{" "}
            {linux.user} logs out. To keep it running, run this on the server:
          </p>
          <Command text={`sudo loginctl enable-linger ${linux.user}`} />
        </div>
      )}
      {linux.selinux === "Enforcing" && (
        <p className="text-[12px] text-muted-foreground">
          SELinux is enforcing. Kelvo labels the binary so systemd can run it.
        </p>
      )}
      {!linux.docker_readable && (
        <p className="text-[12px] text-muted-foreground">
          {linux.user} can’t read the Docker socket, so containers show by short
          id instead of name.
        </p>
      )}
    </div>
  );
}
