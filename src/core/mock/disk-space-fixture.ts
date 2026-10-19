/**
 * The mock disk space tree: a developer's Mac, sized like the spike's
 * (plan/disk-space-report.md). Folders written with a size and no children
 * get a few generated files and a "smaller items" remainder, so every folder
 * opens onto something.
 */

export type SpaceSpec =
  | { t: "dir"; name: string; kids: SpaceSpec[] }
  | { t: "leaf"; name: string; bytes: number; files?: readonly string[] }
  | { t: "file"; name: string; bytes: number }
  | { t: "smaller"; bytes: number; count: number };

const GB = 1e9;
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

/** The boot Data volume, from its root. */
export const BOOT_TREE: SpaceSpec = d("Macintosh HD", [
  d("Users", [
    d("riley", [
      d("Library", [
        d("Developer", [
          d("Xcode", [
            l("DerivedData", 18.4),
            l("iOS DeviceSupport", 13.7),
            l("Archives", 5.9),
            s(0.4, 21),
          ]),
          d("CoreSimulator", [
            l("Devices", 21.2, ["data.img", "runtime.dmg", "dyld.cache"]),
            l("Caches", 3.1),
          ]),
        ]),
        l("Containers", 38.2, ["Docker.raw", "Data.sqlite", "cache.db"]),
        l("Application Support", 26.4, ["Steam", "Code", "Google"]),
        l("Caches", 14.2, ["Homebrew", "com.apple.dt.Xcode", "Google"]),
        l("Mobile Documents", 9.3),
        l("Group Containers", 6.1),
        l("Logs", 1.2),
        s(0.6, 48),
      ]),
      d("Projects", [
        d("opendata", [
          l("data", 28.3, ["census.parquet", "trade.parquet", "acs.duckdb"]),
          l("node_modules", 9.1),
          l(".git", 3.4, ["pack-91ad.pack", "pack-11c0.pack"]),
          s(0.3, 214),
        ]),
        d("kelvo", [
          l("target", 17.2, ["debug", "release", "deps"]),
          l("node_modules", 1.4),
          s(0.1, 380),
        ]),
        l("vitals", 7.1),
        l("sandbox", 2.6),
        s(1.2, 31),
      ]),
      d("Movies", [
        l("Final Cut Projects", 18.1, ["Render Files", "Media.fcpbundle"]),
        f("Screen Recording 2026-09-14.mov", 6.2),
        f("Screen Recording 2026-08-02.mov", 3.9),
        f("demo-4k.mp4", 2.4),
        s(0.4, 12),
      ]),
      l("Downloads", 22.4, [
        "Xcode_26.dmg",
        "ubuntu-26.04-arm64.iso",
        "dataset.zip",
      ]),
      l("Music", 12.3, ["Music Library.musiclibrary", "Podcasts"]),
      l("Documents", 11.1),
      d("Pictures", [l("Photos Library.photoslibrary", 8.4), s(0.6, 40)]),
      l(".rustup", 6.2, ["toolchains", "downloads"]),
      l(".cache", 4.1),
      l(".cargo", 3.2, ["registry", "git"]),
      l("Desktop", 2.9),
      s(0.3, 160),
    ]),
    l("Shared", 0.8),
  ]),
  d("Applications", [
    l("Xcode.app", 12.4, ["Contents"]),
    l("Docker.app", 2.1, ["Contents"]),
    l("Final Cut Pro.app", 5.8, ["Contents"]),
    l("Logic Pro.app", 3.9, ["Contents"]),
    l("Google Chrome.app", 1.4, ["Contents"]),
    l("Slack.app", 0.6, ["Contents"]),
    s(4.2, 63),
  ]),
  d("private", [
    d("var", [
      l("folders", 6.3),
      l("db", 3.8, ["uuidtext", "diagnostics", "Spotlight-V100"]),
      l("log", 1.1),
      s(0.4, 30),
    ]),
  ]),
  d("Library", [
    l("Developer", 8.7, ["CommandLineTools", "CoreSimulator"]),
    l("Application Support", 3.2),
    l("Caches", 1.3),
    s(1.1, 52),
  ]),
  d("opt", [l("homebrew", 9.2, ["Cellar", "Caskroom", "share"])]),
  d("usr", [l("local", 1.4)]),
  l("System", 0.6),
]);

/** An external drive, scanned on its own. */
export const EXTERNAL_TREE: SpaceSpec = d("SanDisk Extreme", [
  l("Backups", 214.2, ["photos-2025.sparsebundle", "laptop-2024.dmg"]),
  l("Footage", 151.6, ["A001.mov", "A002.mov", "B001.mov", "B002.mov"]),
  l("Archive", 38.4),
  s(2.1, 44),
]);

/** Locations on the boot volume macOS protects, as `Users/riley/<rel>`. */
export const BOOT_PROTECTED = [
  ["Desktop", "Desktop"],
  ["Documents", "Documents"],
  ["Downloads", "Downloads"],
  ["Library/Containers", "App containers"],
  ["Library/Group Containers", "Shared app containers"],
  ["Library/Mobile Documents", "iCloud Drive"],
  ["Pictures/Photos Library.photoslibrary", "Photos library"],
] as const;

/** Other volumes in the boot container, by `ATTR_VOL_SPACEUSED`. */
export const BOOT_SIBLINGS = [
  { label: "macOS system", bytes: 13.66 * GB },
  { label: "Preboot volume", bytes: 10.89 * GB },
  { label: "Recovery volume", bytes: 1.54 * GB },
  { label: "Update volume", bytes: 0.08 * GB },
] as const;

/** Snapshots and APFS metadata: the container's used space no file explains. */
export const BOOT_UNATTRIBUTED = 9.4 * GB;
export const BOOT_TOTAL = 994.66 * GB;
export const EXTERNAL_TOTAL = 1000.2 * GB;
export const EXTERNAL_UNATTRIBUTED = 0.6 * GB;

/** Folders the walk could not read, relative to the volume root. */
export const UNREADABLE = [
  "private/var/db/fpsd",
  "private/var/protected",
  "Library/Application Support/com.apple.TCC",
] as const;
