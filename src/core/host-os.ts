/**
 * Copy that depends on the host's OS (`HostInfo.os`). A Mac reads as it
 * always has; a Linux agent (D-115), or an OS this build does not know, is
 * "host" and its own version string.
 */
import { type ByteUnits, formatBytes, formatMarketingMemory } from "./format";
import type { HostInfo, OsKind } from "./generated/bindings";

/** "Mac" for a Mac, otherwise "host": "Not available on this host". */
export function hostNoun(os: OsKind | undefined): "Mac" | "host" {
  return os === undefined || os === "mac_os" ? "Mac" : "host";
}

/** "macOS 27.0.1"; a Linux host's os-release name as it reads. */
export function osLabel(info: Pick<HostInfo, "os" | "os_version">): string {
  return info.os === "mac_os" ? `macOS ${info.os_version}` : info.os_version;
}

/**
 * Installed memory: a Mac's marketing size ("24 GB unified"); elsewhere
 * MemTotal as measured, which the kernel reports below the marketing size
 * ("16.5 GB").
 */
export function memoryTotalLabel(
  info: Pick<HostInfo, "os" | "mem_total_bytes">,
  units?: ByteUnits
): string {
  return info.os === "mac_os"
    ? `${formatMarketingMemory(info.mem_total_bytes)} unified`
    : formatBytes(info.mem_total_bytes, units ? { units } : {});
}
