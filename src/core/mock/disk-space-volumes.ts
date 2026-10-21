/**
 * The volumes each mock host can scan for a space report: the Mac's boot
 * volume and an external drive, or, in the `linux` scenario, linux-tailscale's
 * mounts as `linux/space.rs` lists them from `/proc/self/mountinfo` (D-117):
 * the mount point is the name, nothing is protected, and "/" is the boot
 * volume. Sizes match the `disk.*` series in `mock/linux.ts`.
 */
import type { SpaceVolume } from "@core/generated/bindings";
import {
  BOOT_PROTECTED,
  BOOT_SIBLINGS,
  BOOT_TOTAL,
  BOOT_TREE,
  BOOT_UNATTRIBUTED,
  EXTERNAL_TOTAL,
  EXTERNAL_TREE,
  EXTERNAL_UNATTRIBUTED,
  type SpaceSpec,
  UNREADABLE,
} from "./disk-space-fixture";
import { sum } from "./disk-space-tree";
import type { ScenarioName } from "./fixtures";

/** A scannable volume and what its scan finds. */
export interface MockVolume {
  volume: SpaceVolume;
  tree: SpaceSpec;
  /** Other volumes in the same container (APFS only). */
  siblings: readonly { label: string; bytes: number }[];
  /** Folders the walk could not read: how many, and the first few. */
  unreadable: { count: number; paths: readonly string[] };
}

const HOME = "Users/riley";
const GB = 1e9;
const NONE = { count: 0, paths: [] } as const;

const d = (name: string, kids: SpaceSpec[]): SpaceSpec => ({
  t: "dir",
  name,
  kids,
});
const l = (name: string, gb: number, files?: readonly string[]): SpaceSpec => ({
  t: "leaf",
  name,
  bytes: gb * GB,
  ...(files && { files }),
});
const f = (name: string, gb: number): SpaceSpec => ({
  t: "file",
  name,
  bytes: gb * GB,
});
const s = (gb: number, count: number): SpaceSpec => ({
  t: "smaller",
  bytes: gb * GB,
  count,
});

/** linux-tailscale's root btrfs: containers, Postgres and a media library. */
const LINUX_ROOT_TREE = d("/", [
  d("var", [
    d("lib", [
      d("docker", [
        l("overlay2", 31.4, ["opendata-worker.layer", "postgres.layer"]),
        l("volumes", 12.6, ["opendata-pg.data", "jellyfin-config.data"]),
      ]),
      l("pgsql", 21.8, ["16384.1", "16384", "000000010000002A00000041"]),
      l("jellyfin", 4.2, ["library.db", "transcodes.bin"]),
      s(1.3, 64),
    ]),
    l("log", 2.4, ["journal.archive", "messages", "dnf.log"]),
    l("cache", 3.1, ["dnf-metadata.solv", "libdnf5.cache"]),
  ]),
  d("home", [
    d("riley", [
      l("media", 38.6, ["concert-2025.mkv", "lectures.mkv", "family.mp4"]),
      l("projects", 6.2, ["census.parquet", "node_modules.tar"]),
      l(".cache", 2.9),
      s(0.4, 31),
    ]),
  ]),
  d("usr", [l("lib", 4.8), l("lib64", 1.9), l("share", 2.3), l("bin", 0.6)]),
  d("opt", [l("jellyfin", 0.9)]),
  l("root", 0.3),
  s(0.2, 18),
]);

const LINUX_BOOT_TREE = d("/boot", [
  f("initramfs-6.17.12-400.asahi.fc41.aarch64+16k.img", 0.11),
  f("initramfs-6.16.9-400.asahi.fc41.aarch64+16k.img", 0.1),
  f("vmlinuz-6.17.12-400.asahi.fc41.aarch64+16k", 0.017),
  f("vmlinuz-6.16.9-400.asahi.fc41.aarch64+16k", 0.016),
  l("loader", 0.01),
  s(0.03, 42),
]);

const LINUX_EFI_TREE = d("/boot/efi", [
  l("EFI", 0.06, ["shimaa64.efi", "grubaa64.efi", "mmaa64.efi"]),
  s(0.002, 6),
]);

/** A Linux mount: used is the files plus what the filesystem keeps itself. */
function linux(
  mount: string,
  tree: SpaceSpec,
  totalGb: number,
  metadataGb: number
): MockVolume {
  return {
    volume: {
      mount,
      name: mount,
      is_boot: mount === "/",
      used_bytes: sum(tree, new Set()) + metadataGb * GB,
      total_bytes: totalGb * GB,
      protected: [],
    },
    tree,
    siblings: [],
    unreadable: NONE,
  };
}

function mac(scenarios: readonly ScenarioName[]): MockVolume[] {
  const siblings = BOOT_SIBLINGS.reduce((a, x) => a + x.bytes, 0);
  const boot: MockVolume = {
    volume: {
      mount: "/",
      name: "Macintosh HD",
      is_boot: true,
      used_bytes: sum(BOOT_TREE, new Set()) + siblings + BOOT_UNATTRIBUTED,
      total_bytes: BOOT_TOTAL,
      protected: BOOT_PROTECTED.map(([rel, label]) => ({
        key: `${HOME}/${rel}`,
        label,
      })),
    },
    tree: BOOT_TREE,
    siblings: BOOT_SIBLINGS,
    unreadable: { count: 312, paths: UNREADABLE },
  };
  const external: MockVolume = {
    volume: {
      mount: "/Volumes/SanDisk Extreme",
      name: "SanDisk Extreme",
      is_boot: false,
      used_bytes: sum(EXTERNAL_TREE, new Set()) + EXTERNAL_UNATTRIBUTED,
      total_bytes: EXTERNAL_TOTAL,
      protected: [],
    },
    tree: EXTERNAL_TREE,
    siblings: [],
    unreadable: NONE,
  };
  return scenarios.includes("disk-space-boot-only") ? [boot] : [boot, external];
}

export function mockVolumes(scenarios: readonly ScenarioName[]): MockVolume[] {
  if (!scenarios.includes("linux")) return mac(scenarios);
  return [
    // 141 GB used of 155 GB, as the `disk.*` series say.
    linux("/", LINUX_ROOT_TREE, 155, 5.1),
    linux("/boot", LINUX_BOOT_TREE, 1.0, 0.097),
    linux("/boot/efi", LINUX_EFI_TREE, 0.5, 0.008),
  ];
}
