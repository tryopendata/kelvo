import { formatTemperature, type TemperatureUnits } from "@core/format";
import { gridIntervalMs } from "@core/live-state";
import { windowWords } from "@core/live-window";
import { heatmapBucketMs } from "@core/series-stats";
import { useDeferredValue } from "react";
import { SectionCard } from "~/components/section-card";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useHeld, useLayout, useRingBuckets } from "~/hooks/use-ring";
import { useUnits } from "~/hooks/use-units";
import { useHost } from "~/stores/host-store";
import { HeatMatrix, HeatScaleLegend } from "~/widgets/heat-matrix";
import { sensorLabel, zoneName } from "../_lib/sensors";

/** Fixed domain for temperature (design-system.md "Y scale"), °C. */
const TEMP_SCALE = { min: 20, max: 110 };

function labelValue(key: string): string {
  return key.slice(key.indexOf("=") + 1, -1);
}

/**
 * SoC thermal zones over the chart window (D-091), the same bucketed matrix
 * as per-core load. Rows keep layout order ("Zone 01" first) so they never
 * jump; the hottest zones read from the right column. Temperatures are
 * never colored by value: intensity on the single temperature hue.
 */
export function ZonesCard({ windowMs: windowProp }: { windowMs: number }) {
  // Deferred for the same reason as CoreLoadCard: a window change remounts
  // every cell, which React can then split across frames.
  const windowMs = useDeferredValue(windowProp);
  const bucketMs = heatmapBucketMs(
    windowMs,
    useHost((s) => gridIntervalMs(s.status))
  );
  const units = useUnits().temperature;
  const layout = useLayout();
  const zoneList = layout?.byMetric.get("thermal.zone") ?? [];
  const sensorList = layout?.byMetric.get("thermal.sensor") ?? [];
  // Without SoC zones (a Linux agent, D-116) the named sensors are the rows.
  const zones = zoneList.length > 0 || sensorList.length === 0;
  const zoneKeys = zones ? zoneList : sensorList;
  const sensorKeys = zones ? sensorList : [];
  const now = useHeld([...zoneKeys, ...sensorKeys]);
  const gaps = useGapBands("sensors");
  const { values, endMs } = useRingBuckets(
    zoneKeys,
    bucketMs,
    Math.ceil(windowMs / bucketMs)
  );
  const title = zones ? "SoC thermal zones" : "Temperature sensors";
  const span = `last ${windowWords(windowMs)}`;
  const temp = (c: number) => formatTemperature(c, { units });
  const compact = (c: number | null) =>
    formatTemperature(c, { units, compact: true });
  const rowLabel = (k: string, i: number) =>
    zones ? zoneName(i) : sensorLabel(labelValue(k));
  // The matrix is one image, so the row tooltips' sensor names and the
  // right column's readings go here too.
  const zoneReadings = zoneKeys
    .map((k, i) =>
      zones
        ? `${zoneName(i)} (${labelValue(k)}) ${compact(now[k] ?? null)}`
        : `${rowLabel(k, i)} ${compact(now[k] ?? null)}`
    )
    .join(", ");

  return (
    <SectionCard
      accent="power"
      origin="bl"
      title={`${title}, ${span}`}
      aside={
        <HeatScaleLegend
          accent="temp"
          minLabel={temp(TEMP_SCALE.min)}
          maxLabel={temp(TEMP_SCALE.max)}
        />
      }
      headerAlign="center"
      compactHeader
      className="gap-3"
    >
      <p className="-mt-1 font-normal text-[12px] text-muted-foreground">
        {zones
          ? "Apple doesn't document which zone sits over which core, so zones keep their sensor names."
          : "As the kernel names them."}
      </p>
      <HeatMatrix
        gaps={gaps}
        rows={zoneKeys.map((k, i) => ({
          id: k,
          label: rowLabel(k, i),
          title: labelValue(k),
          now: now[k] ?? null,
          buckets: values[k] ?? [],
        }))}
        scale={TEMP_SCALE}
        formatNow={compact}
        labelWidth={zones ? 44 : 148}
        bucketMs={bucketMs}
        windowMs={windowMs}
        endMs={endMs}
        accent="temp"
        ariaLabel={`${title}, ${span}, ${bucketMs / 1000} second columns. ${zoneReadings}`}
      />
      <OtherSensors
        units={units}
        sensors={sensorKeys.map((k) => ({
          label: sensorLabel(labelValue(k)),
          value: now[k] ?? null,
        }))}
      />
    </SectionCard>
  );
}

/** Battery, SSD and Wi‑Fi temperatures under the zones. */
function OtherSensors({
  sensors,
  units,
}: {
  sensors: readonly { label: string; value: number | null }[];
  units: TemperatureUnits;
}) {
  if (sensors.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-5 border-border-subtle border-t pt-2.5">
      {sensors.map((x) => (
        <span key={x.label} className="font-normal text-[12px] text-fg-subtle">
          {x.label}{" "}
          <span className="figures text-foreground">
            {formatTemperature(x.value, { units })}
          </span>
        </span>
      ))}
    </div>
  );
}
