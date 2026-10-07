import {
  formatEnergy,
  formatHoursMinutes,
  formatWatts,
  formatWattsFine,
} from "@core/format";
import { moduleState } from "@core/module-state";
import { useState } from "react";
import { BatteryDayCard } from "~/components/battery-day-card";
import { PageHeader } from "~/components/page-header";
import { SensorDumpDialog } from "~/components/sensor-dump-dialog";
import { UnsupportedNotice } from "~/components/unsupported-notice";
import {
  UsageAppsCard,
  type UsageTableConfig,
} from "~/components/usage-apps-card";
import { WindowControl } from "~/components/window-control";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useHostRecord } from "~/hooks/use-host-record";
import { useHeld } from "~/hooks/use-ring";
import { useHost } from "~/stores/host-store";
import { usePowerSource } from "~/stores/live-selectors";
import { PowerStackCard } from "./_components/power-stack-card";
import { RingCards } from "./_components/ring-cards";
import { ZonesCard } from "./_components/zones-card";

/** Energy by app over the chart window (D-093, D-099). */
const ENERGY_TABLE: UsageTableConfig = {
  by: "energy",
  noun: "Energy",
  accent: "power",
  columns: [
    {
      label: "Energy",
      format: (u) => formatEnergy(u.energy_j),
      ranked: true,
    },
    { label: "Average", format: (u) => formatWattsFine(u.avg_w) },
  ],
  footnote: (
    <>
      CPU energy per process, as macOS estimates it. GPU, display and other
      users&apos; processes (system daemons) aren&apos;t attributed, so these
      add up to less than system draw.
    </>
  ),
};

/**
 * Power & Sensors (plan 4.10). An unknown chip shows the notice
 * instead (plan 4.17); a Mac without a battery has no battery section and
 * its System card reads "From power adapter". Energy by app (D-093) sits
 * under the charts, over the same window. The cards wait for the chart
 * window (D-091), so they appear together.
 */
export default function PowerRoute() {
  const info = useHostRecord()?.info;
  const caps = useHost((s) => s.capabilities);
  const [dumpOpen, setDumpOpen] = useState(false);
  const hasBattery =
    caps !== null && moduleState(caps, null, "battery") !== "absent";
  const unknownChip =
    caps !== null && moduleState(caps, null, "power") === "unknown_chip";
  const windowMs = useChartWindow()?.windowMs ?? null;

  if (unknownChip) {
    return (
      <div className="flex flex-col gap-4">
        <PageHeader title="Power & Sensors" />
        <UnsupportedNotice
          modelId={info?.model ?? "unknown model"}
          onShareDump={() => setDumpOpen(true)}
        />
        <SensorDumpDialog open={dumpOpen} onOpenChange={setDumpOpen} />
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Power & Sensors"
        subtitle={<PowerSubtitle />}
        actions={<WindowControl />}
      />
      {windowMs !== null && (
        <>
          <RingCards
            topology={info?.cpu_topology ?? []}
            hasBattery={hasBattery}
          />
          <div className="grid grid-cols-[repeat(auto-fit,minmax(340px,1fr))] gap-4">
            <ZonesCard windowMs={windowMs} />
            <PowerStackCard windowMs={windowMs} />
          </div>
          <UsageAppsCard windowMs={windowMs} config={ENERGY_TABLE} />
          {hasBattery && <BatteryDayCard origin="tl" />}
        </>
      )}
    </div>
  );
}

/**
 * The live part of the header. It subscribes on its own so the 1 Hz system
 * draw re-renders this line, not the whole page under it.
 */
function PowerSubtitle() {
  const v = useHeld(["power.system", "battery.time_remaining"]);
  const source = usePowerSource();
  const onBattery = source === "battery";
  const remaining = v["battery.time_remaining"] ?? null;
  return (
    <>
      {onBattery ? "On battery" : "On power adapter"} ·{" "}
      <span className="data-mono">{formatWatts(v["power.system"])}</span> system
      draw
      {onBattery && remaining !== null && (
        <>
          {" "}
          ·{" "}
          <span className="data-mono">
            {formatHoursMinutes(remaining * 60_000)}
          </span>{" "}
          remaining
        </>
      )}
    </>
  );
}
