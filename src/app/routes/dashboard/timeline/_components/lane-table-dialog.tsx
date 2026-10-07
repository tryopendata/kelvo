import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "~/components/ui/dialog";
import { FIELD_LABEL } from "~/widgets/lib/classes";
import type { Bucket } from "../_lib/buckets";
import { formatMetric, type LaneUnits } from "../_lib/format";
import type { Band } from "../_lib/gaps";
import type { LaneDef } from "../_lib/lanes";
import { clock, dayClock } from "../_lib/time";

export interface LaneTableDialogProps {
  lane: { def: LaneDef; series: Record<string, Bucket[]> } | null;
  bands: readonly Band[];
  fromMs: number;
  toMs: number;
  units: LaneUnits;
  onClose: () => void;
}

/** Column group names for lanes that plot two series. */
const SERIES_NAME: Record<string, string> = {
  "power.system": "System",
  "power.cpu": "CPU",
  "net.tx_total": "Up",
  "net.rx_total": "Down",
};

type Row =
  | { kind: "bucket"; t: number; values: (Bucket | undefined)[] }
  | { kind: "gap"; t: number; band: Band };

/**
 * "Show as table" for one lane (design-system.md Accessibility): bucket
 * time with min, avg and max for each plotted series over the visible
 * range, and a row for each gap instead of empty buckets.
 */
export function LaneTableDialog({
  lane,
  bands,
  fromMs,
  toMs,
  units,
  onClose,
}: LaneTableDialogProps) {
  const metrics = lane?.def.metrics.filter((m) => m.plotted) ?? [];
  const rows: Row[] = [];
  if (lane) {
    const byT = new Map<number, (Bucket | undefined)[]>();
    metrics.forEach((m, i) => {
      for (const b of lane.series[m.metric] ?? []) {
        const row = byT.get(b.t) ?? new Array(metrics.length).fill(undefined);
        row[i] = b;
        byT.set(b.t, row);
      }
    });
    for (const [t, values] of byT) rows.push({ kind: "bucket", t, values });
    for (const band of bands) {
      if (band.module === null || band.module === lane.def.module) {
        rows.push({ kind: "gap", t: band.fromMs, band });
      }
    }
    rows.sort((a, b) => a.t - b.t);
  }
  const multi = metrics.length > 1;

  return (
    <Dialog open={lane !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-h-[80vh] grid-rows-[auto_minmax(0,1fr)] sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{lane?.def.label} as a table</DialogTitle>
          <DialogDescription>
            <span className="data-mono">{dayClock(fromMs)}</span> to{" "}
            <span className="data-mono">{dayClock(toMs)}</span>, one row per
            bucket
          </DialogDescription>
        </DialogHeader>
        <div className="min-h-0 overflow-y-auto">
          <table className="w-full border-collapse text-[12px]">
            <thead className="sticky top-0 bg-popover">
              <tr>
                <th scope="col" className={`${FIELD_LABEL} py-1.5 text-left`}>
                  Time
                </th>
                {metrics.flatMap((m) =>
                  ["min", "avg", "max"].map((stat) => (
                    <th
                      key={`${m.metric}-${stat}`}
                      scope="col"
                      className={`${FIELD_LABEL} py-1.5 text-right`}
                    >
                      {multi ? `${SERIES_NAME[m.metric] ?? m.metric} ` : ""}
                      {stat}
                    </th>
                  ))
                )}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) =>
                row.kind === "gap" ? (
                  <tr
                    key={`gap-${row.t}`}
                    className="border-border-subtle border-t"
                  >
                    <td className="data-mono py-1">{clock(row.band.fromMs)}</td>
                    <td
                      colSpan={metrics.length * 3}
                      className="py-1 text-right font-normal text-muted-foreground"
                    >
                      {row.band.label}
                    </td>
                  </tr>
                ) : (
                  <tr key={row.t} className="border-border-subtle border-t">
                    <td className="data-mono py-1">{clock(row.t)}</td>
                    {metrics.flatMap((m, i) => {
                      const b = row.values[i];
                      return (["min", "avg", "max"] as const).map((stat) => (
                        <td
                          key={`${m.metric}-${stat}`}
                          className="data-mono py-1 text-right"
                        >
                          {formatMetric(m.metric, b?.[stat], units)}
                        </td>
                      ));
                    })}
                  </tr>
                )
              )}
            </tbody>
          </table>
        </div>
      </DialogContent>
    </Dialog>
  );
}
