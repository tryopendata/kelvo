/**
 * Sample props for the Overview, CPU, Power and popover widgets. Used by the
 * widget tests and the dev gallery only.
 */
import type { CoreTilesProps } from "../core-tiles";
import type { InlineBarProps } from "../inline-bar";
import type { LegendProps } from "../legend";
import type { MetricCardProps } from "../metric-card";
import type { ProcessListRow } from "../process-list";
import type { ResidencyBarProps } from "../residency-bar";
import type { RingGaugeProps } from "../ring-gauge";
import type { RingStatCardProps } from "../ring-stat-card";
import type { StackBarProps } from "../stack-bar";
import type { StatGridProps } from "../stat-grid";
import type { StatStripProps } from "../stat-strip";

function procs(rows: [string, string, string][]): ProcessListRow[] {
  return rows.map(([initial, name, value], i) => ({
    id: `${name}-${i}`,
    initial,
    name,
    value,
  }));
}

/** Seeded walk: 60 values around `base`. */
function walk(n: number, seed: number, base: number, noise: number) {
  let s = seed;
  const r = () => {
    s = (s * 16807) % 2147483647;
    return (s - 1) / 2147483646;
  };
  let v = base;
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    v += (r() - 0.5) * noise;
    v += (base - v) * 0.2;
    out.push(Math.round(Math.max(1, v) * 10) / 10);
  }
  return out;
}

const T_END = 1_759_617_600_000; // Sun Oct 4 2026 22:40 UTC-ish; any fixed ms epoch

export const OVERVIEW_CARDS: MetricCardProps[] = [
  {
    accent: "cpu",
    origin: "tl",
    title: "CPU",
    subtitle: "10P + 4E",
    ring: { fractions: [0.124, 0.056], value: "18%", label: "Load" },
    bars: [
      { label: "P-cluster", value: "3.2 GHz", fraction: 0.71 },
      { label: "E-cluster", value: "1.0 GHz", fraction: 0.34 },
    ],
    legend: [
      { label: "User", value: "12.4%", step: 1 },
      { label: "System", value: "5.6%", step: 2 },
    ],
    body: {
      kind: "list",
      ariaLabel: "Top processes by CPU",
      rows: procs([
        ["X", "Xcode", "6.2%"],
        ["k", "kernel_task", "3.1%"],
        ["W", "WindowServer", "2.4%"],
        ["S", "Safari", "1.8%"],
        ["n", "node", "1.1%"],
      ]),
    },
    href: "/dashboard/cpu",
  },
  {
    accent: "gpu",
    origin: "tr",
    title: "GPU",
    subtitle: "20-core",
    ring: { fractions: [0.28, 0.08], value: "36%", label: "Load" },
    bars: [
      { label: "Frequency", value: "1.1 GHz", fraction: 0.69 },
      { label: "Power", value: "3.1 W", fraction: 0.16 },
    ],
    legend: [
      { label: "Render", value: "28%", step: 1 },
      { label: "Tiler", value: "8%", step: 2 },
    ],
    body: {
      kind: "stream",
      stream: {
        series: [{ key: "gpu.util", values: walk(60, 9, 36, 10), step: 1 }],
        tEndMs: T_END,
        intervalMs: 1000,
        yMax: 100,
        accent: "gpu",
        height: 96,
        ariaLabel: "GPU, last 60 seconds, 36%",
        windowLabel: "60s",
      },
    },
    href: "/dashboard/gpu",
  },
  {
    accent: "mem",
    origin: "bl",
    title: "Memory",
    subtitle: "24 GB",
    ring: { fractions: [0.48, 0.25], value: "17.6", label: "GB used" },
    bars: [
      { label: "Pressure", value: "42%", fraction: 0.42 },
      { label: "Swap", value: "512 MB", fraction: 0.25 },
    ],
    legend: [
      { label: "App", value: "11.6 GB", step: 1 },
      { label: "Wired + comp.", value: "6.0 GB", step: 2 },
    ],
    body: {
      kind: "list",
      ariaLabel: "Top processes by memory",
      rows: procs([
        ["X", "Xcode", "3.8 GB"],
        ["D", "Docker", "2.6 GB"],
        ["S", "Safari", "1.9 GB"],
        ["F", "Figma", "1.2 GB"],
        ["n", "node", "840 MB"],
      ]),
    },
    href: "/dashboard/memory",
  },
  {
    accent: "power",
    origin: "br",
    title: "Power & Sensors",
    subtitle: "on battery",
    ring: { fractions: [0.43, 0.27], value: "14.8", label: "Watts" },
    bars: [
      { label: "Hottest SoC zone", value: "61 °C", fraction: 0.58 },
      { label: "Fans", value: "1,850 RPM", fraction: 0.32 },
    ],
    legend: [
      { label: "CPU", value: "6.4 W", step: 1 },
      { label: "GPU + DRAM", value: "4.0 W", step: 2 },
    ],
    body: {
      kind: "list",
      ariaLabel: "Top processes by energy impact",
      rows: procs([
        ["X", "Xcode", "412 EI"],
        ["k", "kernel_task", "188 EI"],
        ["S", "Safari", "96 EI"],
        ["F", "Figma", "74 EI"],
        ["W", "WindowServer", "61 EI"],
      ]),
    },
    href: "/dashboard/power",
  },
  {
    accent: "net",
    origin: "tl",
    title: "Network",
    subtitle: "Wi‑Fi 7 · en0",
    ring: { fractions: [0.256, 0.008], value: "26%", label: "Of link" },
    bars: [
      { label: "Download", value: "38.4 MB/s", fraction: 0.64 },
      { label: "Upload", value: "1.2 MB/s", fraction: 0.06 },
    ],
    legend: [
      { label: "Down", value: "307 Mb/s", step: 1 },
      { label: "Up", value: "9.6 Mb/s", step: 2 },
    ],
    body: {
      kind: "list",
      ariaLabel: "Interfaces by total rate",
      rows: procs([
        ["e", "Wi‑Fi · en0", "39.6 MB/s"],
        ["u", "utun4", "1.1 MB/s"],
        ["b", "bridge100", "0.2 MB/s"],
        ["l", "lo0", "0.1 MB/s"],
        ["e", "Ethernet · en6", "0.0 MB/s"],
      ]),
    },
    href: "/dashboard/network",
  },
  {
    accent: "disk",
    origin: "tr",
    title: "Disk",
    subtitle: "APPLE SSD · 1 TB",
    ring: { fractions: [0.33, 0.06], value: "39%", label: "Used" },
    bars: [
      { label: "Read", value: "220 MB/s", fraction: 0.37 },
      { label: "Write", value: "48 MB/s", fraction: 0.08 },
    ],
    legend: [
      { label: "Data", value: "330 GB", step: 1 },
      { label: "System", value: "58 GB", step: 2 },
    ],
    body: {
      kind: "list",
      ariaLabel: "Top processes by disk I/O",
      rows: procs([
        ["X", "Xcode", "142 MB/s"],
        ["D", "Docker", "51 MB/s"],
        ["m", "mds_stores", "18 MB/s"],
        ["S", "Safari", "6 MB/s"],
        ["n", "node", "3 MB/s"],
      ]),
    },
    href: "/dashboard/disk",
  },
];

/** Power card on a Mac without fans (v1-local-monitor.md 4.5). */
export const PASSIVE_COOLING_CARD: MetricCardProps = {
  ...(OVERVIEW_CARDS[3] as MetricCardProps),
  subtitle: "on power adapter",
  bars: [
    { label: "Hottest SoC zone", value: "61 °C", fraction: 0.58 },
    { label: "Fans", value: "Passive cooling", fraction: "none" },
  ],
};

export const RING_GAUGE: RingGaugeProps = {
  fractions: [0.124, 0.056],
  value: "18%",
  label: "Load",
  accent: "cpu",
  size: 72,
};

export const INLINE_BAR: InlineBarProps = {
  label: "P-cluster",
  value: "3.2 GHz",
  fraction: 0.71,
  accent: "cpu",
};

export const POPOVER_CPU_ROWS: InlineBarProps[] = [
  { label: "User", value: "12.4%", fraction: 0.124, layout: "row" },
  {
    label: "System",
    value: "5.6%",
    fraction: 0.056,
    layout: "row",
    rampStep: 2,
  },
];

export const POPOVER_CPU_STREAM = {
  series: [{ key: "cpu.total", values: walk(60, 5, 18, 9), step: 1 as const }],
  tEndMs: T_END,
  intervalMs: 1000,
  yMax: 40,
  accent: "cpu" as const,
  height: 52,
  ariaLabel: "CPU, last 60 seconds, 18%",
  ceilingLabel: "40%",
  windowLabel: "60s",
  gridlines: [20],
};

export const POPOVER_GPU_STREAM = {
  series: [{ key: "gpu.util", values: walk(60, 9, 36, 10), step: 1 as const }],
  tEndMs: T_END,
  intervalMs: 1000,
  yMax: 80,
  accent: "gpu" as const,
  height: 40,
  ariaLabel: "GPU, last 60 seconds, 36%",
};

export const LEGEND: LegendProps = {
  items: [
    { label: "User", value: "12.4%", step: 1 },
    { label: "System", value: "5.6%", step: 2 },
  ],
  accent: "cpu",
};

export const CORE_TILES: CoreTilesProps = {
  clusters: [
    {
      id: "P0",
      name: "P-cluster",
      freq: "3.2 GHz",
      cores: [34, 22, 41, 18, 12, 9, 27, 15, 8, 6].map((load, i) => ({
        id: `P${i}`,
        load,
      })),
    },
    {
      id: "E0",
      name: "E-cluster",
      freq: "1.0 GHz",
      cores: [52, 38, 44, 29].map((load, i) => ({ id: `E${i}`, load })),
    },
  ],
};

/**
 * The second segment is "App", not "Active", so the numbers
 * reconcile with the Overview (v1-local-monitor.md 4.3).
 */
export const MEMORY_STACK: StackBarProps = {
  accent: "mem",
  showLegend: true,
  segments: [
    {
      key: "wired",
      label: "Wired",
      value: "3.9 GB",
      fraction: 3.9 / 24,
      step: 1,
    },
    {
      key: "app",
      label: "App",
      value: "11.6 GB",
      fraction: 11.6 / 24,
      step: 2,
    },
    {
      key: "compressed",
      label: "Compressed",
      value: "2.1 GB",
      fraction: 2.1 / 24,
      step: "hatch",
    },
    {
      key: "cached",
      label: "Cached files",
      value: "4.0 GB",
      fraction: 4 / 24,
      step: 4,
    },
    {
      key: "free",
      label: "Free",
      value: "2.4 GB",
      fraction: 2.4 / 24,
      step: "track",
    },
  ],
  extraLegend: [{ label: "Swap", value: "512 MB" }],
};

export const POWER_STACK_BAR: StackBarProps = {
  accent: "power",
  segments: [
    { key: "cpu", label: "CPU", value: "6.4 W", fraction: 6.4 / 14.8, step: 1 },
    { key: "gpu", label: "GPU", value: "3.1 W", fraction: 3.1 / 14.8, step: 2 },
    { key: "ane", label: "ANE", value: "0.0 W", fraction: 0, step: "hatch" },
    {
      key: "dram",
      label: "DRAM",
      value: "0.9 W",
      fraction: 0.9 / 14.8,
      step: 3,
    },
    {
      key: "rest",
      label: "Rest of system",
      value: "4.4 W",
      fraction: 4.4 / 14.8,
      step: "track",
    },
  ],
};

export const POWER_STAT_GRID: StatGridProps = {
  accent: "power",
  items: [
    { label: "CPU", value: "6.4 W", step: 1 },
    { label: "GPU", value: "3.1 W", step: 2 },
    { label: "ANE", value: "0.0 W", step: "hatch", muted: true },
    { label: "DRAM", value: "0.9 W", step: 3 },
  ],
};

export const GPU_STAT_GRID: StatGridProps = {
  items: [
    { label: "Freq", value: "1.1 GHz" },
    { label: "Power", value: "3.1 W" },
    { label: "Cores", value: "20" },
  ],
};

export const BATTERY_STAT_GRID: StatGridProps = {
  items: [
    { label: "Remaining", value: "6:12" },
    { label: "Health", value: "94%" },
    { label: "Cycles", value: "212" },
  ],
};

export const CPU_STAT_STRIP: StatStripProps = {
  accent: "cpu",
  hero: { label: "Total", value: "18%" },
  items: [
    { label: "User", value: "12.4%", swatch: 1 },
    { label: "System", value: "5.6%", swatch: 2 },
    { label: "Idle", value: "82.0%", muted: true },
    { label: "Load avg", value: "3.42", secondary: "2.98 2.71" },
  ],
};

export const BATTERY_STAT_STRIP: StatStripProps = {
  items: [
    { label: "Charge", value: "87%" },
    { label: "Health", value: "94%" },
    { label: "Cycles", value: "212" },
    { label: "Remaining", value: "6:12" },
    { label: "Full charge", value: "68.2", unit: "of 72.6 Wh" },
  ],
};

export const RING_STAT_CARDS: RingStatCardProps[] = [
  {
    accent: "power",
    origin: "tl",
    title: "CPU",
    description: "Max of SMC CPU group",
    ring: { fractions: [61 / 105], value: "61", label: "°C", accent: "temp" },
    kv: { label: "P-cluster", value: "3.2 GHz" },
  },
  {
    accent: "power",
    origin: "tr",
    title: "GPU",
    description: "Max of SMC GPU group",
    ring: { fractions: [54 / 105], value: "54", label: "°C", accent: "temp" },
    kv: { label: "Frequency", value: "1.1 GHz" },
  },
  {
    accent: "power",
    origin: "bl",
    title: "Fans",
    description: "Left 1,840 · right 1,860",
    ring: {
      fractions: [1850 / 5700],
      value: "1,850",
      label: "RPM",
      accent: "power",
    },
    kv: { label: "Mode", value: "Automatic" },
  },
  {
    accent: "power",
    origin: "br",
    title: "System",
    description: "Drawn from battery",
    ring: {
      fractions: [14.8 / 40],
      value: "14.8",
      label: "W",
      accent: "power",
    },
    kv: { label: "Package", value: "10.4 W" },
  },
];

export const PASSIVE_FANS_CARD: RingStatCardProps = {
  accent: "power",
  origin: "bl",
  title: "Fans",
  description: "Passive cooling",
  ring: null,
  kv: { label: "Mode", value: "No fans" },
};

export const RESIDENCY: ResidencyBarProps[] = [
  {
    cluster: "P-cluster",
    activePct: 41,
    accent: "cpu",
    states: [
      { label: "4.51 GHz", pct: 4 },
      { label: "3.86 GHz", pct: 9 },
      { label: "3.20 GHz", pct: 18 },
      { label: "2.42 GHz", pct: 10 },
      { label: "idle", pct: 59 },
    ],
  },
  {
    cluster: "E-cluster",
    activePct: 59,
    accent: "cpu",
    states: [
      { label: "2.89 GHz", pct: 6 },
      { label: "2.16 GHz", pct: 11 },
      { label: "1.02 GHz", pct: 42 },
      { label: "idle", pct: 41 },
    ],
  },
];
