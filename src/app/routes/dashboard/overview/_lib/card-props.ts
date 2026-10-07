/**
 * Overview card props from live values (plan 4.5). Pure, so
 * the ring, bar and legend math is tested without rendering. Every input is
 * a raw catalog unit; `null` is a gap and renders "—", never 0.
 */
import {
  formatBytes,
  formatPercent,
  formatRate,
  formatTemperature,
  formatWatts,
  MISSING,
  type RateUnits,
  type TemperatureUnits,
} from "@core/format";
import type { LiveProcess, PowerSource } from "@core/generated/bindings";
import { netTotal } from "~/components/process-table";
import type { MetricCardProps } from "~/widgets/metric-card";
import type { ProcessListRow } from "~/widgets/process-list";
import type { StreamAreaProps } from "~/widgets/stream-area";

type CardBase = Omit<MetricCardProps, "origin" | "href" | "onOpen">;

/** `part / whole`, or null when either is missing (an empty track, no fill). */
export function ratio(
  part: number | null,
  whole: number | null
): number | null {
  if (part === null || whole === null || whole <= 0) return null;
  return part / whole;
}

const ghz = (hz: number | null) =>
  hz === null ? MISSING : `${(hz / 1e9).toFixed(1)} GHz`;

/**
 * Top `n` processes by `value`, highest first. Rows without a value drop out;
 * the id is pid plus start time, which survives pid reuse.
 */
export function topProcesses(
  rows: readonly LiveProcess[],
  value: (p: LiveProcess) => number | null,
  format: (v: number) => string,
  n = 5
): ProcessListRow[] {
  return rows
    .map((p) => ({ p, v: value(p) }))
    .filter((r): r is { p: LiveProcess; v: number } => r.v !== null)
    .sort((a, b) => b.v - a.v)
    .slice(0, n)
    .map(({ p, v }) => ({
      id: `${p.pid}:${p.start_time_us}`,
      initial: p.name.slice(0, 1),
      name: p.name,
      value: format(v),
    }));
}

export interface ClusterInput {
  /** "P-cluster", "E-cluster". */
  label: string;
  freqHz: number | null;
  /** The cluster's top DVFS state, Hz. */
  maxHz: number | null;
}

export function cpuCard(input: {
  total: number | null;
  user: number | null;
  system: number | null;
  /** "10P + 4E", from the topology. */
  coresLabel: string;
  /** Logical cores; Overview shows process CPU as a share of the machine. */
  coreCount: number;
  clusters: [ClusterInput, ClusterInput];
  processes: readonly LiveProcess[];
}): CardBase {
  const cores = Math.max(1, input.coreCount);
  return {
    accent: "cpu",
    title: "CPU",
    subtitle: input.coresLabel,
    ring: {
      fractions: [ratio(input.user, 100), ratio(input.system, 100)],
      value: formatPercent(input.total),
      label: "Load",
    },
    bars: input.clusters.map((c) => ({
      label: c.label,
      value: ghz(c.freqHz),
      fraction: ratio(c.freqHz, c.maxHz),
    })) as CardBase["bars"],
    legend: [
      {
        label: "User",
        value: formatPercent(input.user, { decimals: 1 }),
        step: 1,
      },
      {
        label: "System",
        value: formatPercent(input.system, { decimals: 1 }),
        step: 2,
      },
    ],
    body: {
      kind: "list",
      ariaLabel: "Top processes by CPU",
      rows: topProcesses(
        input.processes,
        (p) => (p.cpu_pct === null ? null : p.cpu_pct / cores),
        (v) => formatPercent(v, { decimals: 1 })
      ),
    },
  };
}

/**
 * GPU card. The legend is Render and Tiler, what IOAccelerator reports (plan
 * 4.5, in place of "Compute"). With per-process GPU time (D-085) the
 * body is the top 5 processes by GPU share; without it,
 * a 60 s utilization chart.
 */
export function gpuCard(input: {
  util: number | null;
  render: number | null;
  tiler: number | null;
  freqHz: number | null;
  maxFreqHz: number | null;
  powerW: number | null;
  /** 24 h maximum of `power.gpu`, floored at 5 W. */
  powerScaleW: number;
  /** "20-core" when the host reports GPU cores; empty until it does. */
  subtitle: string;
  /**
   * Process rows when the host has per-process GPU time (D-085), else the
   * chart. Rows without a measured share (the first sample is a baseline)
   * and rows with none drop out.
   */
  body: { processes: readonly LiveProcess[] } | { stream: StreamAreaProps };
}): CardBase {
  return {
    accent: "gpu",
    title: "GPU",
    subtitle: input.subtitle,
    ring: {
      fractions: [ratio(input.render, 100), ratio(input.tiler, 100)],
      value: formatPercent(input.util),
      label: "Load",
    },
    bars: [
      {
        label: "Frequency",
        value: ghz(input.freqHz),
        fraction: ratio(input.freqHz, input.maxFreqHz),
      },
      {
        label: "Power",
        value: formatWatts(input.powerW),
        fraction: ratio(input.powerW, input.powerScaleW),
      },
    ],
    legend: [
      { label: "Render", value: formatPercent(input.render), step: 1 },
      { label: "Tiler", value: formatPercent(input.tiler), step: 2 },
    ],
    body:
      "processes" in input.body
        ? gpuProcessesBody(input.body.processes)
        : { kind: "stream", stream: input.body.stream },
  };
}

function gpuProcessesBody(rows: readonly LiveProcess[]): CardBase["body"] {
  const measured = rows.some((p) => p.gpu_pct !== null);
  return {
    kind: "list",
    ariaLabel: "Top processes by GPU",
    rows: topProcesses(
      rows,
      (p) => (p.gpu_pct === null || p.gpu_pct <= 0 ? null : p.gpu_pct),
      (v) => formatPercent(v, { decimals: 1 })
    ),
    // Rows are the user's processes only (D-045); the first sample after
    // the view opens is a baseline.
    note: measured ? "Your processes only" : "Measuring GPU by process…",
  };
}

export function memoryCard(input: {
  used: number | null;
  app: number | null;
  wired: number | null;
  compressed: number | null;
  pressure: number | null;
  swapUsed: number | null;
  totalBytes: number | null;
  binary: boolean;
  processes: readonly LiveProcess[];
}): CardBase {
  const units = input.binary ? "GiB" : "GB";
  const div = input.binary ? 2 ** 30 : 1e9;
  const wiredComp =
    input.wired === null || input.compressed === null
      ? null
      : input.wired + input.compressed;
  return {
    accent: "mem",
    title: "Memory",
    subtitle:
      input.totalBytes === null ? "" : marketingMemory(input.totalBytes),
    ring: {
      fractions: [
        ratio(input.app, input.totalBytes),
        ratio(wiredComp, input.totalBytes),
      ],
      value: input.used === null ? MISSING : (input.used / div).toFixed(1),
      label: `${units} used`,
    },
    bars: [
      {
        label: "Pressure",
        value: formatPercent(input.pressure),
        fraction: ratio(input.pressure, 100),
      },
      // Swap allocated is not a catalog series, so the bar has no scale.
      { label: "Swap", value: formatBytes(input.swapUsed), fraction: "none" },
    ],
    legend: [
      {
        label: "App",
        value: formatBytes(input.app, { units, decimals: 1 }),
        step: 1,
      },
      {
        label: "Wired + comp.",
        value: formatBytes(wiredComp, { units, decimals: 1 }),
        step: 2,
      },
    ],
    body: {
      kind: "list",
      ariaLabel: "Top processes by memory",
      rows: topProcesses(
        input.processes,
        (p) => p.mem_bytes,
        (v) => formatBytes(v, { units })
      ),
    },
  };
}

/** Plan 4.5: the hottest SoC zone bar runs to 105 °C. */
export const THERMAL_CEILING_C = 105;

export function powerCard(input: {
  system: number | null;
  cpu: number | null;
  gpu: number | null;
  dram: number | null;
  hottestC: number | null;
  fanRpm: number | null;
  fanMax: number | null;
  /** No fans on this Mac (MacBook Air): the bar becomes "Passive cooling". */
  passive: boolean;
  /** `LiveStatus.power_source`; null before the first status. */
  powerSource: PowerSource | null;
  temperature: TemperatureUnits;
  processes: readonly LiveProcess[];
}): CardBase {
  const gpuDram =
    input.gpu === null || input.dram === null ? null : input.gpu + input.dram;
  return {
    accent: "power",
    title: "Power & Sensors",
    subtitle:
      input.powerSource === null
        ? ""
        : input.powerSource === "battery"
          ? "on battery"
          : "on power adapter",
    ring: {
      fractions: [ratio(input.cpu, input.system), ratio(gpuDram, input.system)],
      value: input.system === null ? MISSING : input.system.toFixed(1),
      label: "Watts",
    },
    bars: [
      {
        label: "Hottest SoC zone",
        value: formatTemperature(input.hottestC, { units: input.temperature }),
        fraction: ratio(input.hottestC, THERMAL_CEILING_C),
      },
      input.passive
        ? { label: "Fans", value: "Passive cooling", fraction: "none" }
        : {
            label: "Fans",
            value:
              input.fanRpm === null
                ? MISSING
                : `${Math.round(input.fanRpm).toLocaleString("en-US")} rpm`,
            fraction: ratio(input.fanRpm, input.fanMax),
          },
    ],
    legend: [
      { label: "CPU", value: formatWatts(input.cpu), step: 1 },
      { label: "GPU + DRAM", value: formatWatts(gpuDram), step: 2 },
    ],
    body: {
      kind: "list",
      ariaLabel: "Top processes by energy impact",
      rows: topProcesses(
        input.processes,
        (p) => p.energy,
        (v) => `${v.toFixed(1)} EI`
      ),
    },
  };
}

export interface IfaceRate {
  iface: string;
  rx: number | null;
  tx: number | null;
}

export function networkCard(input: {
  /**
   * The primary interface's name and rates; null when no reported interface
   * carries the default route (a full-tunnel VPN, D-092).
   */
  primary: IfaceRate | null;
  /** `net.link_rate` of the primary interface, bits/s. */
  linkBps: number | null;
  /** `net.rx_total` / `net.tx_total`: every reported interface, bytes/s. */
  total: { rx: number | null; tx: number | null };
  interfaces: readonly IfaceRate[];
  rate: RateUnits;
  /**
   * Process rows when the host has per-process network (D-081), else null
   * and the list shows interfaces. Rows without measured rates (the first
   * sample is a baseline) and rows with no traffic drop out.
   */
  processes: readonly LiveProcess[] | null;
}): CardBase {
  const { rx, tx } = input.total;
  const bits = (v: number | null) => (v === null ? null : v * 8);
  // The ring is the primary interface against its own link; the figures
  // and bars are every interface's traffic.
  const ringDown = ratio(bits(input.primary?.rx ?? null), input.linkBps);
  const ringUp = ratio(bits(input.primary?.tx ?? null), input.linkBps);
  return {
    accent: "net",
    title: "Network",
    subtitle:
      input.primary === null
        ? "All interfaces"
        : `All interfaces · ${input.primary.iface}`,
    ring: {
      fractions: [ringDown, ringUp],
      value: ringDown === null ? MISSING : formatPercent(ringDown * 100),
      label: "Of link",
    },
    bars: [
      {
        label: "Download",
        value: formatRate(rx, { units: input.rate }),
        fraction: ratio(bits(rx), input.linkBps),
      },
      {
        label: "Upload",
        value: formatRate(tx, { units: input.rate }),
        fraction: ratio(bits(tx), input.linkBps),
      },
    ],
    legend: [
      { label: "Down", value: formatRate(rx, { units: "Mbps" }), step: 1 },
      { label: "Up", value: formatRate(tx, { units: "Mbps" }), step: 2 },
    ],
    body:
      input.processes === null
        ? interfacesBody(input.interfaces, input.rate)
        : {
            kind: "list",
            ariaLabel: "Top processes by network rate",
            rows: topProcesses(
              input.processes,
              (p) => {
                const t = netTotal(p);
                return t === null || t <= 0 ? null : t;
              },
              (v) => formatRate(v, { units: input.rate })
            ),
            // NetworkStatistics sees only the user's own flows (D-081).
            note: "Your processes only",
          },
  };
}

/** Without per-process network: interfaces by total rate. */
function interfacesBody(
  interfaces: readonly IfaceRate[],
  rate: RateUnits
): CardBase["body"] {
  const total = (i: IfaceRate) =>
    i.rx === null || i.tx === null ? null : i.rx + i.tx;
  return {
    kind: "list",
    ariaLabel: "Interfaces by total rate",
    rows: interfaces
      .map((i) => ({ i, v: total(i) }))
      .filter((r): r is { i: IfaceRate; v: number } => r.v !== null)
      .sort((a, b) => b.v - a.v)
      .slice(0, 5)
      .map(({ i, v }) => ({
        id: i.iface,
        initial: i.iface.slice(0, 1),
        name: i.iface,
        value: formatRate(v, { units: rate }),
      })),
  };
}

/**
 * Disk card for the boot volume. On APFS every volume reports its
 * container's size, free and used space (`disk.used`, total − free in
 * Rust). A Data and System split is not measured; the legend
 * shows used and free instead.
 */
export function diskCard(input: {
  read: number | null;
  write: number | null;
  totalBytes: number | null;
  freeBytes: number | null;
  usedBytes: number | null;
  /** 24 h maxima of read and write, floored at 500 MB/s. */
  readScale: number;
  writeScale: number;
  rate: RateUnits;
  processes: readonly LiveProcess[];
}): CardBase {
  const used = input.usedBytes;
  const usedFrac = ratio(used, input.totalBytes);
  return {
    accent: "disk",
    title: "Disk",
    subtitle: input.totalBytes === null ? "" : formatBytes(input.totalBytes),
    ring: {
      fractions: [usedFrac],
      value: usedFrac === null ? MISSING : formatPercent(usedFrac * 100),
      label: "Used",
    },
    bars: [
      {
        label: "Read",
        value: formatRate(input.read, { units: input.rate }),
        fraction: ratio(input.read, input.readScale),
      },
      {
        label: "Write",
        value: formatRate(input.write, { units: input.rate }),
        fraction: ratio(input.write, input.writeScale),
      },
    ],
    legend: [
      { label: "Used", value: formatBytes(used), step: 1 },
      { label: "Free", value: formatBytes(input.freeBytes), step: "track" },
    ],
    body: {
      kind: "list",
      ariaLabel: "Top processes by disk I/O",
      rows: topProcesses(
        input.processes,
        (p) =>
          p.disk_read_bps === null && p.disk_write_bps === null
            ? null
            : (p.disk_read_bps ?? 0) + (p.disk_write_bps ?? 0),
        (v) => formatRate(v, { units: input.rate })
      ),
    },
  };
}

/** "10P + 4E" from cluster kinds and core counts. */
export function coresLabel(
  topology: readonly { kind: string; cores: readonly string[] }[]
): string {
  const count = (kind: string) =>
    topology
      .filter((c) => c.kind === kind)
      .reduce((n, c) => n + c.cores.length, 0);
  const p = count("performance");
  const e = count("efficiency");
  if (p === 0 && e === 0) return "";
  return [p ? `${p}P` : null, e ? `${e}E` : null].filter(Boolean).join(" + ");
}

/** "24 GB" for the machine header and Memory card: the marketing size. */
export function marketingMemory(totalBytes: number): string {
  return `${Math.round(totalBytes / 2 ** 30)} GB`;
}
