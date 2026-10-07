import { HistoryChart } from "~/components/charts/history-chart";
import { BatteryHistoryBars } from "~/widgets/battery-history-bars";
import { Card } from "~/widgets/card";
import { ChartAnnotation } from "~/widgets/chart-annotation";
import { CoreHeatmap, HeatScaleLegend } from "~/widgets/core-heatmap";
import { GapBand } from "~/widgets/gap-band";
import { MirrorBars } from "~/widgets/mirror-bars";
import { PowerStack } from "~/widgets/power-stack";
import { StreamArea } from "~/widgets/stream-area";
import { GalleryItem } from "../_components/gallery-item";
import {
  BATTERY_HOURS,
  CORE_HEATMAP,
  CPU_TOTAL,
  CPU_WITH_GAP,
  HISTORY,
  NETWORK_BARS,
  POPOVER_CPU,
  POPOVER_GPU,
  POWER_STACK,
  SLEEP_WINDOW,
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
          <ChartCard id="gc-hist" title="HistoryChart (uPlot)" accent="cpu">
            <HistoryChart {...HISTORY} />
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

      <GalleryItem name="PowerStack · ChartAnnotation" usedIn="Power & Sensors">
        <ChartCard
          id="gc-power"
          title="Power by component, last 10 minutes"
          accent="power"
          aside={
            <span className="data-mono text-[13px]">
              10.4 W{" "}
              <span className="text-[11px] text-muted-foreground">package</span>
            </span>
          }
        >
          <PowerStack {...POWER_STACK} />
        </ChartCard>
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
