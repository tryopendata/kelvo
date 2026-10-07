/**
 * Overview-only selectors over a host's live state. Each returns a primitive
 * or a flat record of primitives, so `useShallow` keeps a frame that did not
 * move them from re-rendering the card.
 */
import type { HostLive } from "@core/live-state";

function keysOf(s: HostLive, metric: string): readonly string[] {
  if (s.layoutNo === null) return [];
  return s.layouts[s.layoutNo]?.byMetric.get(metric) ?? [];
}

/** The value of label `name` in a display-form key `m{a=1,b=2}`. */
export function labelValue(key: string, name: string): string | undefined {
  const open = key.indexOf("{");
  if (open < 0) return undefined;
  for (const pair of key.slice(open + 1, -1).split(",")) {
    const eq = pair.indexOf("=");
    if (pair.slice(0, eq) === name) return pair.slice(eq + 1);
  }
  return undefined;
}

/** `{ "rx|en0": 38.4e6, "tx|en0": 1.2e6, … }` for every interface. */
export function ifaceRates(s: HostLive): Record<string, number | null> {
  const out: Record<string, number | null> = {};
  for (const metric of ["net.rx", "net.tx"] as const) {
    for (const k of keysOf(s, metric)) {
      const iface = labelValue(k, "iface");
      if (iface !== undefined)
        out[`${metric.slice(4)}|${iface}`] = s.held[k] ?? null;
    }
  }
  return out;
}

/** `net.link_rate` of `iface`, bits/s. */
export function linkRate(s: HostLive, iface: string | null): number | null {
  if (iface === null) return null;
  return s.held[`net.link_rate{iface=${iface}}`] ?? null;
}

/**
 * Capacity of volume `vol` (the boot volume is `HostInfo.boot_mounts[0]`,
 * D-092). On APFS every volume reports its container's size and free space.
 */
export function volumeCapacity(
  s: HostLive,
  vol: string | null
): { total: number | null; free: number | null; used: number | null } {
  if (vol === null) return { total: null, free: null, used: null };
  return {
    total: s.held[`disk.total{vol=${vol}}`] ?? null,
    free: s.held[`disk.free{vol=${vol}}`] ?? null,
    used: s.held[`disk.used{vol=${vol}}`] ?? null,
  };
}
