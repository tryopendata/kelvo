import { formatTemperature, type TemperatureUnits } from "@core/format";
import { gridIntervalMs } from "@core/live-state";
import { windowWords } from "@core/live-window";
import { heatmapBucketMs } from "@core/series-stats";
import { useDeferredValue } from "react";
import { SectionCard } from "~/components/section-card";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useHeld, useLayout, useRingBuckets } from "~/hooks/use-ring";
import { useUnits } from "~/hooks/use-units";
import { cn } from "~/lib/utils";
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
export function ZonesCard({
  windowMs: windowProp,
  className,
}: {
  windowMs: number;
  className?: string;
}) {
  // Deferred for the same reason as CoreLoadCard: a window change remounts
  // every cell, which React can then split across frames.
  const windowMs = useDeferredValue(windowProp);
  const bucketMs = heatmapBucketMs(
    windowMs,
    useHost((s) => gridIntervalMs(s.status))
  );
  const units = useUnits().temperature;
  const layout = useLayout();
  const zoneKeys = layout?.byMetric.get("thermal.zone") ?? [];
  const sensorKeys = layout?.byMetric.get("thermal.sensor") ?? [];
  const now = useHeld([...zoneKeys, ...sensorKeys]);
  const gaps = useGapBands("sensors");
  const { values, endMs } = useRingBuckets(
    zoneKeys,
    bucketMs,
    Math.ceil(windowMs / bucketMs)
  );
  const span = `last ${windowWords(windowMs)}`;
  const temp = (c: number) => formatTemperature(c, { units });
  // The matrix is one image, so the row tooltips' sensor names go here too.
  const sensorNames = zoneKeys
    .map((k, i) => `${zoneName(i)}: ${labelValue(k)}`)
    .join(", ");

  return (
    <SectionCard
      accent="power"
      origin="bl"
      title={`SoC thermal zones, ${span}`}
      aside={
        <HeatScaleLegend
          accent="temp"
          minLabel={temp(TEMP_SCALE.min)}
          maxLabel={temp(TEMP_SCALE.max)}
        />
      }
      headerAlign="center"
      compactHeader
      className={cn("col-span-2 gap-3", className)}
    >
      <p className="-mt-1 font-normal text-[12px] text-muted-foreground">
        Apple doesn't document which zone sits over which core, so zones keep
        their sensor names.
      </p>
      <HeatMatrix
        gaps={gaps}
        rows={zoneKeys.map((k, i) => ({
          id: k,
          label: zoneName(i),
          title: labelValue(k),
          now: now[k] ?? null,
          buckets: values[k] ?? [],
        }))}
        scale={TEMP_SCALE}
        formatNow={(c) => formatTemperature(c, { units, compact: true })}
        labelWidth={44}
        bucketMs={bucketMs}
        windowMs={windowMs}
        endMs={endMs}
        accent="temp"
        ariaLabel={`SoC thermal zones, ${span}, ${bucketMs / 1000} second columns. ${sensorNames}`}
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
