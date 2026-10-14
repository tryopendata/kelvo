import { type RateUnit, rateParts } from "@core/format";
import type { LaneData } from "../_hooks/use-timeline-data";
import { bucketAt } from "../_lib/buckets";
import { formatMetric, type LaneUnits } from "../_lib/format";
import type { LaneDef } from "../_lib/lanes";
import type { TooltipRow } from "./crosshair-tooltip";
export function laneValue(
  lane: LaneData | undefined,
  metric: string,
  cursor: number,
  bucketMs: number
) {
  return lane
    ? (bucketAt(lane.series[metric] ?? [], cursor, bucketMs)?.avg ?? null)
    : null;
}
/** "8.2 / 0.4 MB/s": down then up, in the down figure's unit. */
function downUp(
  down: number | null,
  up: number | null,
  units: LaneUnits
): string {
  if (down === null || up === null) {
    return `${formatMetric("net.rx_total", down, units)} / ${formatMetric("net.tx_total", up, units)}`;
  }
  const d = rateParts(down, { units: units.rate });
  const u = rateParts(up, {
    units: units.rate,
    unit: d.unit as RateUnit,
  });
  return `${d.value} / ${u.value} ${d.unit}`;
}

export function laneTooltipRows(
  lanes: LaneData[],
  cursor: number,
  bucketMs: number,
  units: LaneUnits
): TooltipRow[] {
  const value = (lane: LaneData | undefined, metric: string) =>
    laneValue(lane, metric, cursor, bucketMs);
  const byId = (id: LaneDef["id"]) => lanes.find((l) => l.def.id === id);
  const rows: TooltipRow[] = [];
  const add = (
    id: LaneDef["id"],
    label: string,
    metric: string,
    accent: TooltipRow["accent"]
  ) => {
    const lane = byId(id);
    if (lane) {
      rows.push({
        label,
        accent,
        value: formatMetric(metric, value(lane, metric), units),
      });
    }
  };
  add("cpu", "CPU", "cpu.total", "cpu");
  add("gpu", "GPU", "gpu.util", "gpu");
  add("memory", "Memory pressure", "mem.pressure", "mem");
  add("power", "Power (system)", "power.system", "power");
  add("temp", "Hottest SoC zone", "thermal.hottest", "temp");
  const temp = byId("temp");
  if (temp && (temp.series["fan.rpm"]?.length ?? 0) > 0) {
    add("temp", "Fans", "fan.rpm", "temp");
  }
  const net = byId("network");
  if (net) {
    rows.push({
      label: "Network ↓ / ↑",
      accent: "net",
      value: downUp(
        value(net, "net.rx_total"),
        value(net, "net.tx_total"),
        units
      ),
    });
  }

  return rows;
}
