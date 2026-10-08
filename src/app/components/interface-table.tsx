import { formatBytes, formatRate, type RateUnits } from "@core/format";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "~/components/ui/table";

export interface InterfaceRow {
  /** BSD name and row key, "en0". */
  id: string;
  /** "Wi‑Fi", "Ethernet", "VPN", "Bridge". */
  kind: string;
  /** `net.rx{iface}` / `net.tx{iface}`, bytes per second; `null` is a gap. */
  rxBps: number | null;
  txBps: number | null;
  /** Received and sent since boot, bytes. */
  rxBytes: number | null;
  txBytes: number | null;
  /** The interface left the layout (cable pulled, VPN down). */
  disconnected?: boolean;
}

export interface InterfaceTableProps {
  rows: readonly InterfaceRow[];
  units: RateUnits;
}

/**
 * Network interfaces (v1-local-monitor.md 4.11; styled like the CPU page's
 * process table). No IP addresses: they are personal data in screenshots.
 */
export function InterfaceTable({ rows, units }: InterfaceTableProps) {
  return (
    <Table>
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead>Interface</TableHead>
          <TableHead>Kind</TableHead>
          <TableHead className="text-right">Down</TableHead>
          <TableHead className="text-right">Up</TableHead>
          <TableHead className="text-right">Received</TableHead>
          <TableHead className="text-right">Sent</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((r) => (
          <TableRow key={r.id} className="text-[12px]">
            <TableCell className="figures text-foreground">{r.id}</TableCell>
            <TableCell className="font-normal text-fg-subtle">
              {r.disconnected ? (
                <span className="text-muted-foreground">Disconnected</span>
              ) : (
                r.kind
              )}
            </TableCell>
            <TableCell className="figures text-right">
              {formatRate(r.rxBps, { units })}
            </TableCell>
            <TableCell className="figures text-right">
              {formatRate(r.txBps, { units })}
            </TableCell>
            <TableCell className="figures text-right text-fg-subtle">
              {formatBytes(r.rxBytes)}
            </TableCell>
            <TableCell className="figures text-right text-fg-subtle">
              {formatBytes(r.txBytes)}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
}
