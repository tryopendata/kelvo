import { windowLabel } from "@core/live-window";
import { useRef } from "react";
import { SectionCard } from "~/components/section-card";
import { ZoneTable } from "~/components/zone-table";
import { useHeld, useLayout, useRingStats } from "~/hooks/use-ring";
import { useUnits } from "~/hooks/use-units";
import { useHost } from "~/stores/host-store";
import {
  nextZoneOrder,
  sensorLabel,
  type ZoneOrder,
  zoneName,
} from "../_lib/sensors";

function labelValue(key: string): string {
  return key.slice(key.indexOf("=") + 1, -1);
}

/**
 * SoC thermal zones, hottest first, with each zone's range over the chart
 * window (D-091) from the ring. The order is re-sorted at most every 10 s so a row does
 * not jump under the reader; values keep updating in place.
 */
export function ZonesCard({ windowMs }: { windowMs: number }) {
  const units = useUnits();
  const layout = useLayout();
  const zoneList = layout?.byMetric.get("thermal.zone") ?? [];
  const sensorList = layout?.byMetric.get("thermal.sensor") ?? [];
  // Without SoC zones (a Linux agent, D-115) the named sensors are the rows.
  const zones = zoneList.length > 0 || sensorList.length === 0;
  const zoneKeys = zones ? zoneList : sensorList;
  const sensorKeys = zones ? sensorList : [];
  const now = useHeld([...zoneKeys, ...sensorKeys]);
  const range = useRingStats(zoneKeys, windowMs);
  const lastTs = useHost((s) => s.lastTsMs) ?? 0;

  const order = useRef<ZoneOrder | null>(null);
  order.current = nextZoneOrder(
    order.current,
    zoneKeys.map((k) => ({ key: k, now: now[k] ?? null })),
    lastTs
  );
  const position = new Map(zoneKeys.map((k, i) => [k, i]));

  return (
    <SectionCard
      accent="power"
      origin="bl"
      variant="default"
      title={zones ? "SoC thermal zones" : "Temperature sensors"}
    >
      <p className="-mt-3 font-normal text-[12px] text-muted-foreground">
        {zones
          ? "Apple doesn't document which zone sits over which core, so zones keep their sensor names. Hottest first."
          : "As the kernel names them. Hottest first."}
      </p>
      <ZoneTable
        variant={zones ? "zones" : "sensors"}
        units={units.temperature}
        rangeLabel={windowLabel(windowMs)}
        rows={order.current.keys.map((k) => ({
          key: labelValue(k),
          name: zones
            ? zoneName(position.get(k) ?? 0)
            : sensorLabel(labelValue(k)),
          now: now[k] ?? null,
          min: range[k]?.min ?? null,
          max: range[k]?.max ?? null,
        }))}
        extras={sensorKeys.map((k) => ({
          label: sensorLabel(labelValue(k)),
          value: now[k] ?? null,
        }))}
      />
    </SectionCard>
  );
}
