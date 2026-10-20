/**
 * Add host fixtures for Linux servers. An SSH alias containing "linux" or "server" is
 * a Linux host, and the rest of the alias picks the dialog state, so every state
 * renders in one dev server without restarting it:
 *
 *   linux-tailscale    root login, fresh install (the default)
 *   linux-user         a non-root login: user service, no linger, a Docker socket it can't use
 *   linux-update       an older agent is installed
 *   linux-newer        a newer agent than this app is installed: connect, never downgrade
 *   linux-nodocker     a root login on a host without Docker
 *   linux-dup          the agent reports the id of a saved host
 *   linux-lowdisk      too little free space (an error)
 *   linux-conflict     an agent installed for another login (an error)
 *
 * Any other alias is a Mac. Values follow the linux-tailscale probe.
 */
import type {
  CommandError,
  LinuxPreparation,
  RemoteHostPreparation,
} from "@core/generated/bindings";
import { linuxHostInfo } from "./linux";

const GB = 1_000_000_000;

const ERRORS: Record<string, CommandError> = {
  lowdisk: {
    kind: "remote_low_disk",
    message:
      "/var/lib has 1800000000 bytes free; the agent needs 2150000000 (the history budget plus the low-disk reserve)",
  },
  conflict: {
    kind: "remote_agent_conflict",
    message:
      "This host already runs a Kelvo agent installed as user at /home/riley/.local/bin/kelvo-agent; remove it first",
  },
};

type Variant = "fresh" | "user" | "update" | "newer" | "nodocker" | "dup";

/** The app's version in the mock; `linux-newer`'s agent is past it. */
const APP_VERSION = "0.1.0";

export function isLinuxAlias(alias: string): boolean {
  return /linux|server/.test(alias);
}

function rootLinux(): LinuxPreparation {
  const info = linuxHostInfo(0);
  return {
    layout: "system",
    user: "root",
    distro: info.os_version,
    kernel: info.kernel ?? null,
    arch: "aarch64",
    systemd_version: 256,
    linger_ok: true,
    selinux: "Enforcing",
    binary_path: "/usr/local/bin/kelvo-agent",
    data_dir: "/var/lib/kelvo-agent",
    free_bytes: 13.4 * GB,
    needed_bytes: 2.15 * GB,
    budget_mb: 150,
    existing_version: null,
    docker: "readable",
    warnings: [],
    duplicate_of: null,
  };
}

const VARIANTS: Record<
  Variant,
  (duplicateOf: string | null) => Partial<LinuxPreparation>
> = {
  fresh: () => ({}),
  user: () => ({
    layout: "user",
    user: "riley",
    linger_ok: false,
    selinux: null,
    binary_path: "/home/riley/.local/bin/kelvo-agent",
    data_dir: "/home/riley/.local/state/kelvo-agent",
    docker: "unreadable",
    warnings: [
      "riley is not lingering, so the agent stops when riley logs out and starts only at their next login. To keep it running, run `sudo loginctl enable-linger riley`",
    ],
  }),
  update: () => ({ needed_bytes: null, existing_version: "0.0.9" }),
  newer: () => ({ needed_bytes: null, existing_version: "0.2.0" }),
  nodocker: () => ({ docker: "absent" }),
  dup: (duplicateOf) => ({
    needed_bytes: null,
    existing_version: APP_VERSION,
    duplicate_of: duplicateOf,
  }),
};

function variant(alias: string): Variant {
  return (
    (["user", "update", "newer", "nodocker", "dup"] as const).find((v) =>
      alias.includes(v)
    ) ?? "fresh"
  );
}

/** The preparation for a Linux `alias`; `duplicateOf` names the host a `*dup*` alias collides with. */
export function mockLinuxPreparation(
  alias: string,
  duplicateOf: string | null
): { ok: RemoteHostPreparation } | { error: CommandError } {
  const failure = Object.keys(ERRORS).find((key) => alias.includes(key));
  if (failure) return { error: ERRORS[failure] as CommandError };
  const kind = variant(alias);
  const linux = { ...rootLinux(), ...VARIANTS[kind](duplicateOf) };
  return {
    ok: {
      kind: "linux",
      installed: linux.existing_version !== null,
      compatible: kind === "dup" || kind === "newer",
      newer_installed: kind === "newer",
      host_id: kind === "dup" ? "remote-linux-dup" : null,
      name: kind === "user" ? "build-box" : "m1-linux",
      message: null,
      linux,
    },
  };
}
