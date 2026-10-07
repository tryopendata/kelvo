import { formatTemperature, formatWatts, MISSING } from "@core/format";
import { Navigate } from "react-router";
import { BatteryDayCard } from "~/components/battery-day-card";
import { PageHeader } from "~/components/page-header";
import { useBatteryDetail } from "~/hooks/use-battery";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useUnits } from "~/hooks/use-units";
import { useWindowSeries } from "~/hooks/use-window-series";
import { useHost } from "~/stores/host-store";
import { usePowerSource } from "~/stores/live-selectors";
import { RingStatCard } from "~/widgets/ring-stat-card";
import { StatStrip } from "~/widgets/stat-strip";
import { batterySubtitle, powerText, timeField } from "./_lib/battery";

const TEN_MIN_MS = 600_000;
const POWER_KEY = ["battery.power"];

const frac = (pct: number | null) =>
  pct === null ? [] : [Math.min(1, Math.max(0, pct / 100))];

const wh = (v: number | null) => (v === null ? MISSING : v.toFixed(1));

/**
 * Battery (plan 4.13): a row of ring cards in the Power & Sensors style
 * (charge, health, battery power) over the "Battery, last 24 hours" section.
 * The page does not exist on a Mac without a battery.
 */
export default function BatteryRoute() {
  const cap = useHost((s) => s.capabilities?.modules.battery);
  const b = useBatteryDetail();
  const state = usePowerSource() ?? "unknown";
  const units = useUnits();
  // Power ring scale: the 10-minute peak of |battery.power|, snapped to a
  // nice step so the ring does not rescale every sample.
  const series = useWindowSeries(POWER_KEY, TEN_MIN_MS);
  const abs = (series.values["battery.power"] ?? []).map((v) =>
    v === null ? null : Math.abs(v)
  );
  const ceiling = useNiceCeiling(abs, series.tEndMs, 10);

  if (cap === "not_present")
    return <Navigate to="/dashboard/overview" replace />;

  const time = timeField(b, state);
  const power = powerText(b.powerW);

  return (
    <div className="flex flex-col gap-4">
      <PageHeader title="Battery" subtitle={batterySubtitle(b, state)} />
      <div className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-4">
        <RingStatCard
          accent="battery"
          origin="tl"
          title="Charge"
          ring={{
            fractions: frac(b.charge),
            value: b.charge === null ? MISSING : b.charge.toFixed(0),
            label: "%",
            accent: "battery",
          }}
          kv={time}
        />
        <RingStatCard
          accent="battery"
          origin="tr"
          title="Health"
          description="Maximum capacity compared with when new"
          ring={{
            fractions: frac(b.health),
            value: b.health === null ? MISSING : b.health.toFixed(0),
            label: "%",
            accent: "battery",
          }}
          kv={{
            label: "Cycles",
            value: b.cycles === null ? MISSING : String(b.cycles),
          }}
        />
        <RingStatCard
          accent="battery"
          origin="bl"
          title="Power"
          description={power.description}
          ring={
            b.powerW === null
              ? null
              : {
                  fractions: [Math.min(1, Math.abs(b.powerW) / ceiling)],
                  value: power.value,
                  label: "W",
                  accent: "battery",
                }
          }
          kv={{ label: "System", value: formatWatts(b.systemW) }}
        />
      </div>
      <BatteryDayCard
        origin="br"
        aside={
          <StatStrip
            items={[
              {
                label: "Full charge",
                value: wh(b.capacityWh),
                unit: b.designWh === null ? "Wh" : `of ${wh(b.designWh)} Wh`,
              },
              {
                label: "Temperature",
                value: formatTemperature(b.tempC, { units: units.temperature }),
              },
            ]}
          />
        }
      />
    </div>
  );
}
