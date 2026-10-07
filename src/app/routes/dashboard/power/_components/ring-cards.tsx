import {
  formatGhz,
  formatWatts,
  MISSING,
  rpmParts,
  temperatureParts,
  wattsParts,
} from "@core/format";
import type { ClusterInfo } from "@core/generated/bindings";
import { sk } from "@core/series-key";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useHeld, useLayout } from "~/hooks/use-ring";
import { useUnits } from "~/hooks/use-units";
import { useWindowSeries } from "~/hooks/use-window-series";
import { usePowerSource } from "~/stores/live-selectors";
import { RingStatCard } from "~/widgets/ring-stat-card";
import { fanModeLabel, fanSummary } from "../_lib/sensors";

/** Ring scale for SoC temperatures (plan 4.5 uses the same 105 °C top). */
const TEMP_MAX_C = 105;

/**
 * The four ring cards across the top of Power & Sensors: CPU and GPU temperature,
 * fans, and system power. Temperature rings use the temp accent inside the
 * amber card; the value is never recoloured by how hot it is.
 */
export function RingCards({
  topology,
  hasBattery,
}: {
  topology: readonly ClusterInfo[];
  hasBattery: boolean;
}) {
  const units = useUnits();
  const source = usePowerSource();
  const layout = useLayout();
  const fanIds = (layout?.byMetric.get("fan.rpm") ?? []).map((k) =>
    k.slice(k.indexOf("=") + 1, -1)
  );
  const pCluster = topology.find((c) => c.kind !== "efficiency")?.name;
  const pFreqKey = pCluster
    ? sk("cpu.cluster.freq", { cluster: pCluster })
    : "";
  const v = useHeld([
    "thermal.cpu",
    "thermal.gpu",
    "gpu.freq",
    "power.system",
    "power.package",
    "fan.mode",
    pFreqKey,
    ...fanIds.flatMap((f) => [
      sk("fan.rpm", { fan: f }),
      sk("fan.max", { fan: f }),
    ]),
  ]);
  const at = (k: string) => v[k] ?? null;
  const system = useWindowSeries(["power.system"], 600_000);
  const systemMax = useNiceCeiling(
    system.values["power.system"] ?? [],
    system.tEndMs,
    40
  );

  const temp = (c: number | null) =>
    c === null
      ? { value: MISSING, unit: `°${units.temperature}` }
      : temperatureParts(c, { units: units.temperature });
  const cpuT = temp(at("thermal.cpu"));
  const gpuT = temp(at("thermal.gpu"));

  const rpms = fanIds.map((f) => at(sk("fan.rpm", { fan: f })));
  const maxes = fanIds.map((f) => at(sk("fan.max", { fan: f })));
  const rpmNow = rpms.reduce<number | null>(
    (m, x) => (x === null ? m : m === null ? x : Math.max(m, x)),
    null
  );
  const rpmMax = maxes.reduce<number | null>(
    (m, x) => (x === null ? m : m === null ? x : Math.max(m, x)),
    null
  );

  const sys = at("power.system");
  const fromAdapter =
    !hasBattery || source === "adapter" || source === "charging";

  return (
    <div className="grid grid-cols-[repeat(auto-fit,minmax(210px,1fr))] gap-4">
      <RingStatCard
        accent="power"
        origin="tl"
        title="CPU"
        description="Max of SMC CPU group"
        ring={{
          fractions: [frac(at("thermal.cpu"), TEMP_MAX_C)],
          value: cpuT.value,
          label: cpuT.unit,
          accent: "temp",
        }}
        kv={{ label: "P-cluster", value: formatGhz(at(pFreqKey)) }}
      />
      <RingStatCard
        accent="power"
        origin="tr"
        title="GPU"
        description="Max of SMC GPU group"
        ring={{
          fractions: [frac(at("thermal.gpu"), TEMP_MAX_C)],
          value: gpuT.value,
          label: gpuT.unit,
          accent: "temp",
        }}
        kv={{ label: "Frequency", value: formatGhz(at("gpu.freq")) }}
      />
      {fanIds.length === 0 ? (
        <RingStatCard
          accent="power"
          origin="bl"
          title="Fans"
          description="Passive cooling"
          ring={null}
          kv={{ label: "Fans", value: "None" }}
        />
      ) : (
        <RingStatCard
          accent="power"
          origin="bl"
          title="Fans"
          description={fanSummary(rpms)}
          ring={{
            fractions: [frac(rpmNow, rpmMax)],
            value: rpmNow === null ? MISSING : rpmParts(rpmNow).value,
            label: "RPM",
            accent: "power",
          }}
          kv={{ label: "Mode", value: fanModeLabel(at("fan.mode")) }}
        />
      )}
      <RingStatCard
        accent="power"
        origin="br"
        title="System"
        description={fromAdapter ? "From power adapter" : "Drawn from battery"}
        ring={{
          fractions: [frac(sys, systemMax)],
          value: sys === null ? MISSING : wattsParts(sys).value,
          label: "W",
          accent: "power",
        }}
        kv={{ label: "Package", value: formatWatts(at("power.package")) }}
      />
    </div>
  );
}

function frac(v: number | null, max: number | null): number {
  if (v === null || max === null || max <= 0) return 0;
  return Math.min(1, Math.max(0, v / max));
}
