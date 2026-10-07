import { clamp01 } from "@core/chart-math";
import { type ByteUnits, formatBytes } from "@core/format";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "~/components/ui/table";

export interface VolumeRow {
  /** `vol` label value and row key. */
  id: string;
  /** "Macintosh HD", "Data". */
  name: string;
  usedBytes: number | null;
  freeBytes: number | null;
  totalBytes: number | null;
  disconnected?: boolean;
}

export interface VolumeTableProps {
  /** Boot container first. */
  rows: readonly VolumeRow[];
  units: ByteUnits;
}

/**
 * Disk volumes (v1-local-monitor.md 4.12): used, free, size and a
 * usage bar in the Disk accent.
 */
export function VolumeTable({ rows, units }: VolumeTableProps) {
  return (
    <Table>
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead>Volume</TableHead>
          <TableHead className="text-right">Used</TableHead>
          <TableHead className="text-right">Free</TableHead>
          <TableHead className="text-right">Size</TableHead>
          <TableHead className="w-40">
            <span className="sr-only">Usage</span>
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((r) => {
          const frac =
            r.usedBytes != null && r.totalBytes
              ? clamp01(r.usedBytes / r.totalBytes)
              : null;
          return (
            <TableRow key={r.id} className="text-[12px]">
              <TableCell className="text-foreground">
                {r.name}
                {r.disconnected && (
                  <span className="ml-2 text-muted-foreground">
                    Disconnected
                  </span>
                )}
              </TableCell>
              <TableCell className="data-mono text-right">
                {formatBytes(r.usedBytes, { units })}
              </TableCell>
              <TableCell className="data-mono text-right">
                {formatBytes(r.freeBytes, { units })}
              </TableCell>
              <TableCell className="data-mono text-right text-fg-subtle">
                {formatBytes(r.totalBytes, { units })}
              </TableCell>
              <TableCell>
                <span className="block h-1 overflow-hidden rounded-full bg-track">
                  {frac != null && (
                    <span
                      className="block h-full origin-left rounded-full bg-disk"
                      style={{ transform: `scaleX(${frac})` }}
                    />
                  )}
                </span>
              </TableCell>
            </TableRow>
          );
        })}
      </TableBody>
    </Table>
  );
}
