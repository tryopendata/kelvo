/**
 * Overview card props for a host without the macOS-only series a Linux agent
 * leaves out (D-115): no memory composition or pressure, no power split by
 * component, no per-process energy and no SoC hottest zone. Each starts from
 * the Mac card and replaces the parts that would otherwise read "—".
 */
import { ratio } from "@core/chart-math";
import { formatBytes, formatPercent, formatTemperature } from "@core/format";
import { type CardBase, memoryCard, powerCard } from "./card-props";

/**
 * Memory from `mem.used`, `mem.cached` and `mem.free` (used + cached + free
 * is MemTotal on Linux): the ring is used and cached, the first bar is
 * `mem.psi{stall=some}`, and the total is MemTotal as measured rather than
 * a marketing size.
 */
export function usedMemoryCard(
  input: Omit<
    Parameters<typeof memoryCard>[0],
    "app" | "wired" | "compressed" | "pressure"
  > & { cached: number | null; psiSome: number | null }
): CardBase {
  const units = input.binary ? "GiB" : "GB";
  const base = memoryCard({
    ...input,
    app: null,
    wired: null,
    compressed: null,
    pressure: null,
  });
  return {
    ...base,
    subtitle:
      input.totalBytes === null ? "" : formatBytes(input.totalBytes, { units }),
    ring: {
      ...base.ring,
      fractions: [
        ratio(input.used, input.totalBytes),
        ratio(input.cached, input.totalBytes),
      ],
    },
    bars: [
      {
        label: "Pressure stall",
        value: formatPercent(input.psiSome, { decimals: 1 }),
        fraction: ratio(input.psiSome, 100),
      },
      base.bars[1],
    ],
    legend: [
      {
        label: "Used",
        value: formatBytes(input.used, { units, decimals: 1 }),
        step: 1,
      },
      {
        label: "Cached",
        value: formatBytes(input.cached, { units, decimals: 1 }),
        step: 2,
      },
    ],
  };
}

/**
 * Power from `power.system` and the `thermal.sensor` readings: the ring is
 * the system draw alone, the temperature bar the hottest sensor, and the
 * list the sensors hottest first, in place of top processes by energy.
 */
export function sensorPowerCard(
  input: Omit<
    Parameters<typeof powerCard>[0],
    "cpu" | "gpu" | "dram" | "hottestC" | "processes"
  > & { sensors: readonly { name: string; c: number | null }[] }
): CardBase {
  const measured = input.sensors
    .filter((s): s is { name: string; c: number } => s.c !== null)
    .sort((a, b) => b.c - a.c);
  const base = powerCard({
    ...input,
    cpu: null,
    gpu: null,
    dram: null,
    hottestC: measured[0]?.c ?? null,
    processes: [],
  });
  return {
    ...base,
    ring: { ...base.ring, fractions: [null] },
    bars: [{ ...base.bars[0], label: "Hottest sensor" }, base.bars[1]],
    legend: [],
    body: {
      kind: "list",
      ariaLabel: "Temperature sensors, hottest first",
      rows: measured.slice(0, 5).map((s) => ({
        id: s.name,
        initial: s.name.slice(0, 1),
        name: s.name,
        value: formatTemperature(s.c, { units: input.temperature }),
      })),
    },
  };
}
