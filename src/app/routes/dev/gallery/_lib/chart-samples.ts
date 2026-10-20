import {
  mockContainer,
  mockDockerSnapshot,
  mockHistoryPoints,
} from "@core/mock/docker";
import type { BatteryHistoryBarsProps } from "~/widgets/battery-history-bars";
import type { CoreHeatmapProps } from "~/widgets/core-heatmap";
import type { HeatMatrixRow } from "~/widgets/heat-matrix";
import {
  PERCENT_GRID,
  PERCENT_Y_TICKS,
  windowTicks,
} from "~/widgets/lib/chart-labels";
import type { MirrorBarsProps } from "~/widgets/mirror-bars";
import type { StreamAreaProps } from "~/widgets/stream-area";

/*
 * Chart sample data from seeded generators, so the gallery draws the same
 * shapes every run.
 */

/** Park–Miller generator. */
function rng(seed: number) {
  let s = seed;
  return () => {
    s = (s * 16807) % 2147483647;
    return (s - 1) / 2147483646;
  };
}

type Bump = [center: number, height: number, width: number];

function series(
  n: number,
  seed: number,
  base: number,
  noise: number,
  { lo = 1, hi = Number.POSITIVE_INFINITY, bumps = [] as Bump[] } = {}
): number[] {
  const r = rng(seed);
  let v = base;
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    v += (r() - 0.5) * noise;
    v += (base - v) * 0.2;
    let x = v;
    for (const [c, h, w] of bumps) x += h * Math.exp(-(((i - c) / w) ** 2));
    out.push(Math.min(hi, Math.max(lo, x)));
  }
  return out;
}

function setLast(values: number[], v: number): number[] {
  values[values.length - 1] = v;
  return values;
}

/** Fixed "now" so screenshots are stable: Sun Oct 4 2026, 22:40 local. */
export const NOW_MS = new Date(2026, 9, 4, 22, 40, 0).getTime();

// Popover CPU (ceiling 40%) and GPU charts, 60 s.
export const POPOVER_CPU: StreamAreaProps = {
  series: [
    { key: "total", values: setLast(series(60, 5, 18, 9), 18), step: 1 },
  ],
  tEndMs: NOW_MS,
  intervalMs: 1000,
  yMax: 40,
  accent: "cpu",
  height: 52,
  ariaLabel: "CPU, last 60 seconds",
  ceilingLabel: "40%",
  windowLabel: "60s",
  gridlines: [20],
};

export const POPOVER_GPU: StreamAreaProps = {
  series: [
    { key: "util", values: setLast(series(60, 9, 36, 10), 36), step: 1 },
  ],
  tEndMs: NOW_MS,
  intervalMs: 1000,
  yMax: 80,
  accent: "gpu",
  height: 40,
  ariaLabel: "GPU, last 60 seconds",
};

// CPU page: total and system, 120 samples over 60 s, fixed 0–100.
const total = setLast(series(120, 3, 18, 8, { lo: 0.5, hi: 100 }), 18);
const system = setLast(
  total.map((v, i) => v * (0.28 + 0.06 * Math.sin(i / 7))),
  5.6
);

export const CPU_TOTAL: StreamAreaProps = {
  series: [
    { key: "total", values: total, step: 1 },
    { key: "system", values: system, step: 2 },
  ],
  tEndMs: NOW_MS,
  intervalMs: 500,
  yMax: 100,
  accent: "cpu",
  height: 220,
  ariaLabel: "CPU user and system, last 60 seconds",
  gridlines: PERCENT_GRID,
  yTicks: PERCENT_Y_TICKS,
  xTicks: windowTicks(60_000, 5),
};

// CPU over an hour with a sleep gap between samples 22 and 51.
const SLEEP_FROM = 22;
const SLEEP_TO = 51;
const hourCpu = series(61, 13, 22, 14);
export const SLEEP_WINDOW = {
  fromMs: NOW_MS - 60 * 60_000,
  toMs: NOW_MS,
  gapFromMs: NOW_MS - (60 - SLEEP_FROM) * 60_000,
  gapToMs: NOW_MS - (60 - SLEEP_TO) * 60_000,
};

export const CPU_WITH_GAP: StreamAreaProps = {
  series: [
    {
      key: "total",
      values: hourCpu.map((v, i) =>
        i > SLEEP_FROM && i < SLEEP_TO ? null : v
      ),
      step: 1,
    },
  ],
  tEndMs: NOW_MS,
  intervalMs: 60_000,
  yMax: 100,
  accent: "cpu",
  height: 72,
  ariaLabel: "CPU, last hour, asleep 11:02 to 11:31",
  gridlines: [33, 66],
};

// Popover network, 48 s of mirrored bars.
const netR = rng(31);
const net = Array.from({ length: 48 }, (_, i) => {
  const burst = i > 30 && i < 40 ? 1 : 0.55;
  return {
    up: Math.round(2 + netR() * 10 * (i % 9 === 0 ? 1.6 : 1)),
    dn: Math.round(4 + netR() * 24 * burst),
  };
});
net[47] = { up: 6, dn: 26 };

export const NETWORK_BARS: MirrorBarsProps = {
  up: net.map((b) => b.up),
  down: net.map((b) => b.dn),
  intervalMs: 1000,
  tEndMs: NOW_MS,
  accent: "net",
  ariaLabel: "Upload above the line, download below, last 48 seconds",
  // Raw pixel heights: 18 px and 30 px are the ceilings.
  upMax: 18,
  downMax: 30,
};

// CPU page: per-core load, 60 columns of 10 s. The first four columns are
// before app start, so they are hatched.
const heatR = rng(77);
const NOW_LOAD = [34, 22, 41, 18, 12, 9, 27, 15, 8, 6, 52, 38, 44, 29];
export const CORE_HEATMAP: CoreHeatmapProps = {
  cores: NOW_LOAD.map((cur, k) => {
    const isE = k >= 10;
    const buckets: (number | null)[] = [];
    for (let i = 0; i < 60; i++) {
      const t = 59 - i;
      let x = cur + (heatR() - 0.5) * 30 + (t > 38 && t < 46 && !isE ? 40 : 0);
      x = Math.max(0, Math.min(100, x));
      if (i === 59) x = cur;
      buckets.push(i < 4 ? null : Math.round(x));
    }
    return {
      id: isE ? `E${k - 10}` : `P${k}`,
      cluster: isE ? ("E" as const) : ("P" as const),
      now: cur,
      buckets,
    };
  }),
  bucketMs: 10_000,
  windowMs: 600_000,
  ariaLabel: "Per-core load, last 10 minutes",
};

// Power & Sensors: SoC thermal zones, 60 columns of 10 s in °C. Zone 03
// warms through a build in the middle of the window; the first four columns
// are before app start.
const zoneR = rng(91);
const ZONE_NOW = [52, 49, 61, 47, 55, 50, 58, 46, 53, 48];
const ZONE_SENSORS = [
  "PMU tdie1",
  "PMU tdie2",
  "PMU tdie3",
  "PMU tdie4",
  "PMU tdie5",
  "PMU tdie6",
  "PMU2 tdie1",
  "PMU2 tdie2",
  "PMU2 tdie3",
  "PMU2 tdie4",
];
export const THERMAL_ZONES: HeatMatrixRow[] = ZONE_NOW.map((cur, k) => {
  const buckets: (number | null)[] = [];
  for (let i = 0; i < 60; i++) {
    const t = 59 - i;
    let x = cur + (zoneR() - 0.5) * 6 + (t > 20 && t < 40 ? 22 - k : 0);
    if (i === 59) x = cur;
    buckets.push(i < 4 ? null : Math.round(x * 10) / 10);
  }
  return {
    id: ZONE_SENSORS[k] as string,
    label: `Zone ${String(k + 1).padStart(2, "0")}`,
    title: ZONE_SENSORS[k] as string,
    now: cur,
    buckets,
  };
});

// Power draw: system with the top apps as lines, 120 samples over 10 minutes.
const N = 120;
const systemW = setLast(
  series(N, 5, 14.8, 3, { lo: 2, bumps: [[70, 6, 6]] }),
  14.8
);
/** Apps hold one value per 10 s bucket (two 5 s slots); the first minute is unsampled. */
const appW = (seed: number, base: number, noise: number, bumps: Bump[] = []) =>
  series(N / 2, seed, base, noise, { lo: 0.05, bumps }).flatMap((v, i) =>
    i < 6 ? [null, null] : [v, v]
  );
const APPS: [name: string, values: (number | null)[]][] = [
  ["Google Chrome", appW(21, 2.6, 1.2, [[35, 3, 3]])],
  ["Xcode", appW(23, 1.4, 0.8)],
  ["WindowServer", appW(25, 0.9, 0.3)],
  ["Slack", appW(27, 0.4, 0.2)],
  ["Music", appW(29, 0.2, 0.1)],
];
const APP_STEPS = [2, 3, 4, 2, 3] as const;

export const POWER_DRAW_APPS = APPS.map(([name]) => name);

/** `focused` is an app name: that line at 2 px, the other apps dimmed. */
export function powerDraw(focused: string | null): StreamAreaProps {
  return {
    series: [
      { key: "system", values: systemW, step: 1, weight: 2 },
      ...APPS.map(([name, values], i) => ({
        key: name,
        values,
        step: APP_STEPS[i] ?? 2,
        fill: false,
        weight: focused === name ? 2 : 1,
        opacity: focused === null || focused === name ? 1 : 0.3,
      })),
    ],
    tEndMs: NOW_MS,
    intervalMs: 5000,
    yMax: 30,
    accent: "power",
    height: 220,
    ariaLabel: "System power with the top apps, last 10 minutes",
    gridlines: [15, 30],
    yTicks: [
      { value: 30, label: "30W" },
      { value: 15, label: "15" },
    ],
    xTicks: windowTicks(600_000, 5),
    annotations: [
      {
        tsMs: NOW_MS - (N - 1 - 70) * 5000,
        label: "Package 21 W · Xcode build",
      },
    ],
  };
}

// Battery, 24 hours starting at 23:00 yesterday.
const LEVELS = [
  68, 64, 60, 66, 74, 80, 80, 90, 100, 100, 92, 83, 75, 66, 60, 78, 94, 90, 82,
  74, 84, 96, 98, 87,
];
const CHARGING = [
  0, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0,
];
const DAY_START = new Date(2026, 9, 3, 23, 0, 0).getTime();
export const BATTERY_HOURS: BatteryHistoryBarsProps = {
  hours: LEVELS.map((charge, i) => ({
    tsMs: DAY_START + i * 3_600_000,
    charge,
    charging: CHARGING[i] === 1,
  })),
  annotations: [
    {
      tsMs: DAY_START + 5 * 3_600_000,
      label: "Optimized charging: held at 80%",
    },
  ],
};

/** Six categorical identities, a genuine gap, partial points and a one-minute peak. */
export const DOCKER_COMPARISON = Array.from({ length: 6 }, (_, index) => {
  const container = mockContainer(index, NOW_MS);
  return {
    container,
    slot: index + 1,
    points: mockHistoryPoints(
      {
        engine_id: container.engine_id,
        container_id: container.id,
        from_ms: NOW_MS - 86_400_000,
        to_ms: NOW_MS,
        max_points: 360,
      },
      container
    ),
  };
});
export const DOCKER_NOTICE = {
  ...mockDockerSnapshot(6, NOW_MS),
  persistence: {
    ...mockDockerSnapshot(6, NOW_MS).persistence,
    status: "write_failed" as const,
  },
};
