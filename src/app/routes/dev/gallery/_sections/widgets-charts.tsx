import { formatTemperature } from "@core/format";
import { useState } from "react";
import { BatteryHistoryBars } from "~/widgets/battery-history-bars";
import { Card } from "~/widgets/card";
import { ChartAnnotation } from "~/widgets/chart-annotation";
import { CoreHeatmap } from "~/widgets/core-heatmap";
import { GapBand } from "~/widgets/gap-band";
import { HeatMatrix, HeatScaleLegend } from "~/widgets/heat-matrix";
import { Legend } from "~/widgets/legend";
import { MirrorBars } from "~/widgets/mirror-bars";
import { StreamArea } from "~/widgets/stream-area";
import { GalleryItem } from "../_components/gallery-item";
import {
  BATTERY_HOURS,
  CORE_HEATMAP,
  CPU_TOTAL,
  CPU_WITH_GAP,
  NETWORK_BARS,
  POPOVER_CPU,
  POPOVER_GPU,
  POWER_DRAW_APPS,
  powerDraw,
  SLEEP_WINDOW,
  THERMAL_ZONES,
} from "../_lib/chart-samples";

function ChartCard({
  id,
  title,
  accent,
  aside,
  children,
}: {
  id: string;
  title: string;
  accent: "cpu" | "power" | "battery" | "net" | "gpu";
  aside?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <Card
      accent={accent}
      variant="chart"
      labelledBy={id}
      className="flex flex-col gap-3 p-4"
    >
      <div className="flex items-center gap-3">
        <h3 id={id} className="flex-1 font-[590] text-[13px]">
          {title}
        </h3>
        {aside}
      </div>
      {children}
    </Card>
  );
}

/** Live and history charts, gap bands and annotations (widgets/). */
export function WidgetsChartsSection() {
  return (
    <div className="flex flex-col gap-8">
      <div className="flex flex-wrap items-start gap-8">
        <GalleryItem
          name="StreamArea · MirrorBars"
          usedIn="Popover"
          width={340}
          vibrant
        >
          <Card
            accent="cpu"
            variant="chart"
            labelledBy="gc-cpu"
            className="flex flex-col gap-2 p-3"
          >
            <h3 id="gc-cpu" className="font-[590] text-[12px]">
              CPU
            </h3>
            <StreamArea {...POPOVER_CPU} />
            <div className="h-3" />
          </Card>
          <Card
            accent="gpu"
            variant="chart"
            labelledBy="gc-gpu"
            className="flex flex-col gap-2 p-3"
          >
            <h3 id="gc-gpu" className="font-[590] text-[12px]">
              GPU
            </h3>
            <StreamArea {...POPOVER_GPU} />
          </Card>
          <Card
            accent="net"
            variant="chart"
            labelledBy="gc-net"
            className="flex flex-col gap-2 p-3"
          >
            <h3 id="gc-net" className="font-[590] text-[12px]">
              Network
            </h3>
            <MirrorBars {...NETWORK_BARS} />
          </Card>
        </GalleryItem>

        <GalleryItem
          name="StreamArea · GapBand"
          usedIn="Empty and gap states"
          width={560}
        >
          <ChartCard id="gc-gap" title="CPU, last hour" accent="cpu">
            <div className="relative">
              <StreamArea {...CPU_WITH_GAP} />
              <GapBand
                fromMs={SLEEP_WINDOW.gapFromMs}
                toMs={SLEEP_WINDOW.gapToMs}
                label="Asleep 11:02–11:31 · not interpolated"
                rangeFromMs={SLEEP_WINDOW.fromMs}
                rangeToMs={SLEEP_WINDOW.toMs}
              />
            </div>
            <div className="h-3" />
          </ChartCard>
          <GalleryItem name="GapBand · standalone" usedIn="Gap states">
            <div className="h-16">
              <GapBand fromMs={0} toMs={1} label="Paused" />
            </div>
          </GalleryItem>
        </GalleryItem>
      </div>

      <GalleryItem name="StreamArea · large" usedIn="CPU">
        <ChartCard id="gc-total" title="CPU total" accent="cpu">
          <StreamArea {...CPU_TOTAL} />
        </ChartCard>
      </GalleryItem>

      <GalleryItem name="CoreHeatmap" usedIn="CPU">
        <ChartCard
          id="gc-heat"
          title="Per-core load, last 10 minutes"
          accent="cpu"
          aside={<HeatScaleLegend />}
        >
          <CoreHeatmap {...CORE_HEATMAP} />
        </ChartCard>
      </GalleryItem>

      <GalleryItem name="HeatMatrix · temperature" usedIn="Power & Sensors">
        <ChartCard
          id="gc-zones"
          title="SoC thermal zones, last 10 minutes"
          accent="power"
          aside={
            <HeatScaleLegend accent="temp" minLabel="20 °C" maxLabel="110 °C" />
          }
        >
          <HeatMatrix
            rows={THERMAL_ZONES}
            scale={{ min: 20, max: 110 }}
            formatNow={(c) => formatTemperature(c, { compact: true })}
            labelWidth={44}
            bucketMs={10_000}
            windowMs={600_000}
            accent="temp"
            ariaLabel="SoC thermal zones, last 10 minutes"
          />
        </ChartCard>
      </GalleryItem>

      <GalleryItem
        name="StreamArea · lines over area · ChartAnnotation"
        usedIn="Power & Sensors"
      >
        <PowerDrawSample />
      </GalleryItem>

      <GalleryItem name="BatteryHistoryBars" usedIn="Power & Sensors">
        <ChartCard id="gc-batt" title="Battery, last 24 hours" accent="battery">
          <BatteryHistoryBars {...BATTERY_HOURS} />
        </ChartCard>
      </GalleryItem>

      <GalleryItem name="ChartAnnotation" usedIn="Power & Sensors">
        <div className="flex items-center gap-6">
          <ChartAnnotation tsMs={0} label="ANE 1.4 W · Photos face analysis" />
          <ChartAnnotation
            tsMs={0}
            label="Optimized charging: held at 80%"
            variant="pill"
          />
        </div>
      </GalleryItem>
    </div>
  );
}

/** The power draw chart: hover or click an app to focus its line. */
function PowerDrawSample() {
  const [focused, setFocused] = useState<string | null>(null);
  const steps = [2, 3, 4, 2, 3] as const;
  return (
    <ChartCard id="gc-power" title="Power draw, last 10 minutes" accent="power">
      <Legend
        items={[
          { label: "System", step: 1 },
          ...POWER_DRAW_APPS.map((name, i) => ({
            label: name,
            value: `${(2.6 / (i + 1)).toFixed(1)} W`,
            step: steps[i] ?? 2,
            dashed: i >= 3,
            onFocus: () => setFocused(name),
            onBlur: () => setFocused(null),
            onClick: () => setFocused((f) => (f === name ? null : name)),
            pressed: focused === name,
          })),
        ]}
      />
      <StreamArea {...powerDraw(focused)} />
    </ChartCard>
  );
}
