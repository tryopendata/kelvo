import { formatRate, MISSING } from "@core/format";
import type { LiveProcess } from "@core/generated/bindings";
import { sk } from "@core/series-key";
import type { InterfaceRow } from "~/components/interface-table";

/**
 * One row per interface in the layout, in layout order so rows do not jump.
 * Kind and since-boot totals are not in the schema yet (D-045 keeps them as
 * collector methods), so they render as missing rather than guessed from the
 * BSD name.
 */
export function interfaceRows(
  ifaces: readonly string[],
  held: Record<string, number | null>
): InterfaceRow[] {
  return ifaces.map((id) => ({
    id,
    kind: MISSING,
    rxBps: held[sk("net.rx", { iface: id })] ?? null,
    txBps: held[sk("net.tx", { iface: id })] ?? null,
    rxBytes: null,
    txBytes: null,
  }));
}

/** Download as a share of the link, percent; null without both figures. */
export function ofLink(
  rxBps: number | null,
  linkBitsPerSec: number | null
): number | null {
  if (rxBps === null || linkBitsPerSec === null || linkBitsPerSec <= 0) {
    return null;
  }
  return ((rxBps * 8) / linkBitsPerSec) * 100;
}

/** Header subtitle: "en0 · 1.2 Gb/s link". Interface kind arrives with the schema field. */
export function networkSubtitle(
  primary: string | null,
  linkBitsPerSec: number | null
): string {
  if (primary === null) return "No active interface";
  if (linkBitsPerSec === null) return primary;
  return `${primary} · ${formatRate(linkBitsPerSec / 8, { units: "Mbps" })} link`;
}

/**
 * Process rows with measured traffic: a rate on both sides (not the
 * baseline sample, D-081) and something sent or received.
 */
export function measuredTraffic(rows: readonly LiveProcess[]): LiveProcess[] {
  return rows.filter(
    (p) =>
      p.net_rx_bps !== null &&
      p.net_tx_bps !== null &&
      p.net_rx_bps + p.net_tx_bps > 0
  );
}
