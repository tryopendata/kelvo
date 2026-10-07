import { Card } from "~/widgets/card";
import { CoreTiles } from "~/widgets/core-tiles";
import { InitialChip } from "~/widgets/initial-chip";
import { InlineBar } from "~/widgets/inline-bar";
import { Legend } from "~/widgets/legend";
import {
  BATTERY_STAT_GRID,
  BATTERY_STAT_STRIP,
  CORE_TILES,
  CPU_STAT_STRIP,
  GPU_STAT_GRID,
  INLINE_BAR,
  LEGEND,
  MEMORY_STACK,
  OVERVIEW_CARDS,
  PASSIVE_COOLING_CARD,
  PASSIVE_FANS_CARD,
  POPOVER_CPU_ROWS,
  POPOVER_CPU_STREAM,
  POPOVER_GPU_STREAM,
  POWER_STACK_BAR,
  POWER_STAT_GRID,
  RESIDENCY,
  RING_GAUGE,
  RING_STAT_CARDS,
} from "~/widgets/lib/sample-props";
import { MetricCard } from "~/widgets/metric-card";
import { ModuleCard } from "~/widgets/module-card";
import { ProcessList } from "~/widgets/process-list";
import { ResidencyBar } from "~/widgets/residency-bar";
import { RingGauge } from "~/widgets/ring-gauge";
import { RingStatCard } from "~/widgets/ring-stat-card";
import { StackBar } from "~/widgets/stack-bar";
import { StatGrid } from "~/widgets/stat-grid";
import { StatStrip } from "~/widgets/stat-strip";
import { StatusPill } from "~/widgets/status-pill";
import { StreamArea } from "~/widgets/stream-area";
import { GalleryItem } from "../_components/gallery-item";

const POPOVER_W = 340;
const cpuList = OVERVIEW_CARDS[0]?.body;

/** Cards, gauges, bars, legends, stat cells, lists and pills (widgets/). */
export function WidgetsCardsSection() {
  return (
    <div className="flex flex-col gap-8">
      <GalleryItem name="MetricCard" usedIn="Overview">
        <div className="grid grid-cols-3 gap-4">
          {OVERVIEW_CARDS.map((card) => (
            <MetricCard key={card.title} {...card} onOpen={() => {}} />
          ))}
        </div>
      </GalleryItem>

      <div className="flex flex-wrap items-start gap-8">
        <GalleryItem
          name="MetricCard · passive cooling"
          usedIn="Overview"
          width={340}
        >
          <MetricCard {...PASSIVE_COOLING_CARD} href={undefined} />
        </GalleryItem>

        <GalleryItem name="ModuleCard" usedIn="Popover" vibrant>
          <div className="flex flex-col gap-2" style={{ width: POPOVER_W }}>
            <ModuleCard accent="cpu" title="CPU" value="18%">
              <StreamArea {...POPOVER_CPU_STREAM} />
              <div className="flex flex-col gap-[5px] pt-1.5">
                {POPOVER_CPU_ROWS.map((row) => (
                  <InlineBar key={row.label} {...row} />
                ))}
              </div>
            </ModuleCard>
            <ModuleCard
              accent="cpu"
              title="Cores"
              subtitle="% load"
              subtitleStyle="label"
            >
              <CoreTiles {...CORE_TILES} />
            </ModuleCard>
            <ModuleCard
              accent="mem"
              title="Memory"
              value="17.6"
              unit=" / 24 GB"
            >
              <InlineBar
                label="Pressure · normal"
                value="42%"
                fraction={0.42}
                layout="wide"
              />
              <StackBar {...MEMORY_STACK} />
            </ModuleCard>
            <ModuleCard accent="gpu" title="GPU" value="36%">
              <StreamArea {...POPOVER_GPU_STREAM} />
              <StatGrid {...GPU_STAT_GRID} />
            </ModuleCard>
            <ModuleCard
              accent="power"
              title="Power"
              value="14.8 W"
              unit=" system"
            >
              <StackBar {...POWER_STACK_BAR} />
              <StatGrid {...POWER_STAT_GRID} />
            </ModuleCard>
            <ModuleCard accent="battery" title="Battery" value="87%">
              <InlineBar
                label="Charge"
                value="87%"
                fraction={0.87}
                layout="wide"
              />
              <StatGrid {...BATTERY_STAT_GRID} />
            </ModuleCard>
          </div>
        </GalleryItem>

        <div className="flex flex-col gap-8">
          <GalleryItem
            name="RingGauge"
            usedIn="Overview · Power & Sensors · CPU"
          >
            <div className="flex items-center gap-6">
              <RingGauge {...RING_GAUGE} />
              <RingGauge
                fractions={[14.8 / 40]}
                value="14.8"
                label="W"
                accent="power"
                size={88}
              />
              <RingGauge
                fractions={[3.2 / 4.51]}
                value="3.20"
                label="GHz"
                accent="cpu"
                size={112}
              />
            </div>
          </GalleryItem>
          <GalleryItem name="InlineBar" usedIn="Overview" width={240}>
            <InlineBar {...INLINE_BAR} />
            <InlineBar label="Fans" value="Passive cooling" fraction="none" />
          </GalleryItem>
          <GalleryItem name="Legend" usedIn="Overview">
            <Legend {...LEGEND} />
          </GalleryItem>
          <GalleryItem name="ProcessList" usedIn="Overview" width={300}>
            {cpuList?.kind === "list" && (
              <ProcessList rows={cpuList.rows} ariaLabel={cpuList.ariaLabel} />
            )}
          </GalleryItem>
          <GalleryItem
            name="StatusPill · InitialChip"
            usedIn="Popover · Overview"
          >
            <div className="flex items-center gap-3">
              <StatusPill state="live" label="Live" />
              <StatusPill state="live" label="1s" />
              <StatusPill state="paused" label="Paused" />
              <StatusPill state="stale" label="Stale" />
              <InitialChip text="X" />
              <InitialChip text="k" />
            </div>
          </GalleryItem>
          <GalleryItem name="Card" usedIn="design-system Card anatomy">
            <div className="flex gap-3">
              <Card
                accent="disk"
                origin="tr"
                labelledBy="g-card-a"
                className="p-4"
              >
                <h3 id="g-card-a" className="font-[590] text-[13px]">
                  Default, 9%
                </h3>
              </Card>
              <Card
                accent="disk"
                variant="chart"
                labelledBy="g-card-b"
                className="p-4"
              >
                <h3 id="g-card-b" className="font-[590] text-[13px]">
                  Chart, 6%
                </h3>
              </Card>
            </div>
          </GalleryItem>
        </div>
      </div>

      <GalleryItem name="StatStrip" usedIn="CPU · Power & Sensors">
        <div className="flex flex-col gap-6">
          <StatStrip {...CPU_STAT_STRIP} />
          <StatStrip {...BATTERY_STAT_STRIP} />
        </div>
      </GalleryItem>

      <div className="flex flex-wrap items-start gap-8">
        <GalleryItem name="ResidencyBar" usedIn="CPU" width={320}>
          <div className="flex flex-col gap-4">
            {RESIDENCY.map((r) => (
              <ResidencyBar key={r.cluster} {...r} />
            ))}
          </div>
        </GalleryItem>
      </div>

      <GalleryItem name="RingStatCard" usedIn="Power & Sensors">
        <div className="grid grid-cols-4 gap-4">
          {RING_STAT_CARDS.map((card) => (
            <RingStatCard key={card.title} {...card} />
          ))}
        </div>
        <div className="grid grid-cols-4 gap-4">
          <RingStatCard {...PASSIVE_FANS_CARD} />
        </div>
      </GalleryItem>
    </div>
  );
}
