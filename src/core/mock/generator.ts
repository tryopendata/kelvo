/**
 * Seeded series generator for the mock transport. Every series walks around
 * a fixed sample value (the Overview cards, the CPU page's cores and
 * residency, the Power page's zones and power, the popover), and a backfill
 * ends exactly on those values, so a freshly opened mock window always shows
 * the same figures. CPU total and GPU util use fixed seeds, so their 60 s
 * backfill is always the same chart.
 */
import {
  HOLD_FACTOR,
  type LiveProcess,
  METRIC_PERIODS_MS,
  type Module,
  type SeriesKey,
} from "@core/generated/bindings";
import { seriesKey, seriesKeyString } from "@core/series-key";
import {
  BACKGROUND_PROCESSES,
  E_CORES,
  IDLE_PROCESSES,
  P_CORES,
  PROCESSES,
  type ScenarioFlags,
  WIDE_LAYOUT_SERIES,
} from "./fixtures";
import { clampWalk, rng, type WalkSpec, walkStep } from "./rng";

export interface SeriesSpec extends WalkSpec {
  key: SeriesKey;
  module: Module;
  seed: number;
  /** Sampled every `cadence` ticks (catalog 6.1); `null` in between. */
  cadence: number;
  /**
   * Ticks between samples while no window shows the series: 10 for the
   * engine's adaptive collectors (IOReport, network, disk; D-061), else
   * `cadence`.
   */
  idleCadence: number;
  /**
   * A total over these specs' indices (`net.rx_total`, D-092): the sum of
   * their values on the tick, a gap when any of them is, as the engine's
   * collector computes it.
   */
  sumOf?: number[];
  /**
   * A collector with a period in time, not ticks (`self.cpu`'s catalog
   * period, `METRIC_PERIODS_MS`): it
   * samples every `periodMs` or every tick, whichever is slower, as the
   * engine does.
   */
  periodMs?: number;
  /**
   * Part of `base` that is work done per tick at 1 s, so it scales with the
   * tick rate (Kelvo's own CPU); the rest does not.
   */
  perTick?: number;
}

/**
 * Series whose collectors slow to every 10 s with only the tray open: the
 * IOReport collector (cluster, GPU state and energy power), and network and
 * disk rates, which the default menu bar does not show.
 */
const ADAPTIVE_METRICS = new Set([
  "cpu.cluster.freq",
  "cpu.cluster.active",
  "cpu.cluster.residency",
  "cpu.cluster.power",
  "gpu.freq",
  "gpu.residency",
  "power.gpu",
  "power.ane",
  "power.dram",
  "power.package",
  "net.rx",
  "net.tx",
  "net.rx_total",
  "net.tx_total",
  "disk.read",
  "disk.write",
  "disk.read_total",
  "disk.write_total",
]);

const GHZ = 1e9;

/** P cores, then E cores, current % load. */
const CORE_LOAD = [34, 22, 41, 18, 12, 9, 27, 15, 8, 6, 52, 38, 44, 29];

/** "Cluster residency", % of the last 60 s per DVFS state. */
const RESIDENCY: Record<string, Record<string, number>> = {
  P0: { 4512: 4, 3864: 9, 3204: 18, 2420: 10, 1924: 0, 1260: 0, idle: 59 },
  E0: { 2892: 6, 2160: 11, 1500: 0, 1020: 42, idle: 41 },
};

/** "SoC thermal zones", hottest first. */
const ZONES: [string, number][] = [
  ["PMU tdie4", 61],
  ["PMU tdie1", 59],
  ["PMU tdie6", 58],
  ["PMU2 tdie2", 56],
  ["PMU tdie3", 55],
  ["PMU2 tdie1", 53],
  ["PMU tdie5", 52],
  ["PMU2 tdie4", 50],
  ["PMU tdie2", 49],
  ["PMU2 tdie3", 48],
];

export function buildSpecs(flags: ScenarioFlags): SeriesSpec[] {
  const specs: SeriesSpec[] = [];
  let seed = 1000;
  const add = (
    module: Module,
    metric: string,
    labels: Record<string, string>,
    base: number,
    opts: Partial<Omit<SeriesSpec, "key" | "module" | "base">> = {}
  ) => {
    specs.push({
      key: seriesKey(metric, labels),
      module,
      base,
      noise: opts.noise ?? 0,
      min: opts.min ?? 0,
      max: opts.max,
      seed: opts.seed ?? seed++,
      cadence: opts.cadence ?? 1,
      periodMs: opts.periodMs,
      perTick: opts.perTick,
      idleCadence: ADAPTIVE_METRICS.has(metric) ? 10 : (opts.cadence ?? 1),
    });
  };
  const pct = { min: 0, max: 100 };

  // CPU. Total uses series(60, 5, 18, 9).
  add("cpu", "cpu.total", {}, 18, { noise: 9, seed: 5, min: 1, max: 100 });
  add("cpu", "cpu.user", {}, 12.4, { noise: 6, ...pct });
  add("cpu", "cpu.system", {}, 5.6, { noise: 3, ...pct });
  [...P_CORES, ...E_CORES].forEach((core, i) => {
    add("cpu", "cpu.load", { core }, CORE_LOAD[i] ?? 10, {
      noise: 22,
      ...pct,
    });
  });
  for (const [window, v] of [
    ["1", 3.42],
    ["5", 3.1],
    ["15", 2.88],
  ] as const) {
    add("cpu", "cpu.loadavg", { window }, v, { noise: 0.3, cadence: 5 });
  }
  add("cpu", "cpu.cluster.freq", { cluster: "P0" }, 3.2 * GHZ, {
    noise: 0.5 * GHZ,
    min: 1.26 * GHZ,
    max: 4.512 * GHZ,
  });
  add("cpu", "cpu.cluster.freq", { cluster: "E0" }, 1.02 * GHZ, {
    noise: 0.3 * GHZ,
    min: 1.02 * GHZ,
    max: 2.892 * GHZ,
  });
  add("cpu", "cpu.cluster.active", { cluster: "P0" }, 41, {
    noise: 10,
    ...pct,
  });
  add("cpu", "cpu.cluster.active", { cluster: "E0" }, 59, {
    noise: 10,
    ...pct,
  });
  add("cpu", "cpu.cluster.power", { cluster: "P0" }, 5.8, { noise: 1.5 });
  // SMC CPU power (D-054) has no E-cluster key, so no E-cluster series.
  if (flags.cpuPowerSource === null) {
    add("cpu", "cpu.cluster.power", { cluster: "E0" }, 0.6, { noise: 0.2 });
  }
  for (const [cluster, states] of Object.entries(RESIDENCY)) {
    for (const [state, v] of Object.entries(states)) {
      add("cpu", "cpu.cluster.residency", { cluster, state }, v, {
        noise: v > 0 ? 3 : 0,
        ...pct,
      });
    }
  }

  // GPU. Util uses series(60, 9, 36, 10).
  add("gpu", "gpu.util", {}, 36, { noise: 10, seed: 9, min: 1, max: 100 });
  add("gpu", "gpu.render", {}, 28, { noise: 8, ...pct });
  add("gpu", "gpu.tiler", {}, 8, { noise: 3, ...pct });
  add("gpu", "gpu.freq", {}, 1.1 * GHZ, {
    noise: 0.2 * GHZ,
    min: 0.338 * GHZ,
    max: 1.6 * GHZ,
  });
  for (const [state, v] of [
    ["1578", 6],
    ["1398", 12],
    ["1098", 18],
    ["720", 10],
    ["idle", 54],
  ] as const) {
    add("gpu", "gpu.residency", { state }, v, { noise: 3, ...pct });
  }

  // Memory, in bytes (decimal GB).
  const memNoise = { noise: 0.05e9 };
  add("memory", "mem.used", {}, 17.6e9, memNoise);
  add("memory", "mem.app", {}, 11.6e9, memNoise);
  add("memory", "mem.wired", {}, 3.9e9, memNoise);
  add("memory", "mem.compressed", {}, 2.1e9, memNoise);
  add("memory", "mem.cached", {}, 4.0e9, memNoise);
  add("memory", "mem.free", {}, 2.4e9, memNoise);
  add("memory", "mem.pressure", {}, 42, { noise: 2, ...pct });
  add("memory", "mem.pressure_level", {}, 0);
  add("memory", "mem.swap_used", {}, 512e6);
  add("memory", "mem.swap_in", {}, 0);
  add("memory", "mem.swap_out", {}, 0);

  // Power.
  add("power", "power.cpu", {}, 6.4, { noise: 2.2 });
  add("power", "power.gpu", {}, 3.1, { noise: 1.2 });
  add("power", "power.ane", {}, 0);
  add("power", "power.dram", {}, 0.9, { noise: 0.2 });
  add("power", "power.package", {}, 10.4, { noise: 2.5 });
  add("power", "power.system", {}, 14.8, { noise: 3 });
  if (flags.cpuPowerSource !== null) {
    add("power", "power.cpu_source", {}, flags.cpuPowerSource);
  }

  // Sensors: hidden on an unknown chip; no fans on a MacBook Air.
  if (!flags.unknownChip) {
    for (const [sensor, v] of ZONES) {
      add("sensors", "thermal.zone", { sensor }, v, { noise: 1.5, cadence: 5 });
    }
    add("sensors", "thermal.cpu", {}, 61, { noise: 1.5, cadence: 5 });
    add("sensors", "thermal.gpu", {}, 54, { noise: 1.5, cadence: 5 });
    add("sensors", "thermal.hottest", {}, 61, { noise: 1.5, cadence: 5 });
    for (const [name, v] of [
      ["Battery", 31],
      ["SSD (NAND)", 38],
      ["Wi-Fi module", 44],
    ] as const) {
      add("sensors", "thermal.sensor", { name }, v, { noise: 0.5, cadence: 5 });
    }
    add("sensors", "thermal.state", {}, 0, { cadence: 2 });
    if (!flags.noFans) {
      add("sensors", "fan.rpm", { fan: "0" }, 1840, { noise: 60, cadence: 2 });
      add("sensors", "fan.rpm", { fan: "1" }, 1860, { noise: 60, cadence: 2 });
      add("sensors", "fan.max", { fan: "0" }, 5700, { cadence: 60 });
      add("sensors", "fan.max", { fan: "1" }, 5700, { cadence: 60 });
      add("sensors", "fan.mode", {}, 0, { cadence: 60 });
    }
  }

  // Network: 38.4 MB/s down is 26% of a 1.2 Gb/s link.
  add("network", "net.rx", { iface: "en0" }, 38.4e6, { noise: 18e6 });
  add("network", "net.tx", { iface: "en0" }, 1.2e6, { noise: 0.8e6 });
  add("network", "net.link_rate", { iface: "en0" }, 1.2e9, { cadence: 60 });
  if (flags.vpn) {
    add("network", "net.rx", { iface: "en7" }, 2.5e6, { noise: 1e6 });
    add("network", "net.tx", { iface: "en7" }, 0.4e6, { noise: 0.2e6 });
    add("network", "net.link_rate", { iface: "en7" }, 1e9, { cadence: 60 });
  }
  // Totals over the reported parts, appended after them as the collectors do.
  const total = (module: Module, metric: string, part: string) => {
    const sumOf = specs.flatMap((s, i) => (s.key.metric === part ? [i] : []));
    // Live values are derived; base and noise shape the mock's history.
    const of = (f: (s: SeriesSpec) => number) =>
      sumOf.reduce((n, i) => n + f(specs[i] as SeriesSpec), 0);
    add(
      module,
      metric,
      {},
      of((s) => s.base),
      { noise: of((s) => s.noise) }
    );
    (specs[specs.length - 1] as SeriesSpec).sumOf = sumOf;
  };
  total("network", "net.rx_total", "net.rx");
  total("network", "net.tx_total", "net.tx");

  // Disk: 1 TB container, 612 GB free, Data 330 GB, System 58 GB.
  add("disk", "disk.read", { dev: "disk3" }, 220e6, { noise: 90e6 });
  add("disk", "disk.write", { dev: "disk3" }, 48e6, { noise: 25e6 });
  total("disk", "disk.read_total", "disk.read");
  total("disk", "disk.write_total", "disk.write");
  // Every APFS volume reports its container's figures, and Rust's
  // `disk.used` is total − free (a Data/System split is not
  // measured), so both volumes read 388 GB used.
  for (const vol of ["/System/Volumes/Data", "/"]) {
    add("disk", "disk.used", { vol }, 1000e9 - 612e9, { cadence: 60 });
    add("disk", "disk.free", { vol }, 612e9, { cadence: 60 });
    add("disk", "disk.total", { vol }, 1000e9, { cadence: 60 });
  }

  // Battery.
  if (!flags.noBattery) {
    const b = (metric: string, v: number, cadence: number) =>
      add("battery", metric, {}, v, { cadence, min: -100 });
    b("battery.charge", 87, 10);
    b("battery.charging", 0, 10);
    b("battery.external", 0, 10);
    b("battery.time_remaining", 372, 10);
    b("battery.health", 94, 60);
    b("battery.cycles", 212, 60);
    b("battery.capacity_wh", 68.2, 60);
    b("battery.design_wh", 72.6, 60);
    b("battery.power", -14.8, 10);
    b("battery.temp", 31, 10);
  }

  // Mostly per-tick work: 0.4% at 1 s, under 0.05% at 30 s.
  add("cpu", "self.cpu", {}, 0.4, {
    noise: 0.1,
    periodMs: METRIC_PERIODS_MS["self.cpu"],
    perTick: 0.37,
  });

  // Without the SMC map, CPU power comes from IOReport's PMP counters and
  // slows down with it; the SMC keys are read every tick (D-054).
  for (const spec of specs) {
    const smc = flags.cpuPowerSource !== null;
    if (spec.key.metric === "power.cpu" && !smc) spec.idleCadence = 10;
    if (spec.key.metric === "cpu.cluster.power" && smc) {
      spec.idleCadence = spec.cadence;
    }
  }

  // A full Mac's layout for the perf gate: series no screen reads, so the
  // cost is the channel's and the ring's, not extra chart work.
  if (flags.wideLayout) {
    for (let i = 0; specs.length < WIDE_LAYOUT_SERIES; i++) {
      add("sensors", "mock.pad", { i: String(i) }, 40, { noise: 1 });
    }
  }
  return specs;
}

export interface GeneratedRow {
  values: (number | null)[];
  held: (number | null)[];
  /** Ticks between this row's samples of each series. */
  cadences: number[];
}

/**
 * Advances every series one tick at a time. `values` holds what was measured
 * on this tick (null between a slow series' samples); `held` the latest
 * measured value of each series until it is older than its hold
 * (`HOLD_FACTOR` times its period, the engine's `STALE_NUM / STALE_DEN`),
 * then null (D-047).
 */
export class MockGenerator {
  readonly specs: SeriesSpec[];
  readonly keys: SeriesKey[];
  private readonly state: { v: number; r: () => number }[];
  private readonly last: (number | null)[];
  /** Tick of each series' last sample, and its hold in ticks then. */
  private readonly lastTick: number[];
  private readonly holdTicks: number[];
  /** Series whose reads fail: every tick is a gap. */
  readonly failing = new Set<number>();
  private tick = 0;
  /** No window shows anything: adaptive series run at `idleCadence`. */
  trayOnly = false;
  /** The sampling interval the next tick is taken at. */
  intervalMs = 1000;

  constructor(flags: ScenarioFlags) {
    this.specs = buildSpecs(flags);
    this.keys = this.specs.map((s) => s.key);
    this.state = this.specs.map((s) => ({ v: s.base, r: rng(s.seed) }));
    this.last = this.specs.map(() => null);
    this.lastTick = this.specs.map(() => 0);
    this.holdTicks = this.specs.map(() => 0);
  }

  seriesPerModule(): Record<string, number> {
    const out: Record<string, number> = {};
    for (const s of this.specs) out[s.module] = (out[s.module] ?? 0) + 1;
    return out;
  }

  indexOf(key: string): number {
    return this.keys.findIndex((k) => seriesKeyString(k) === key);
  }

  /** One tick. `land` forces each series onto its sample value. */
  next(land = false): GeneratedRow {
    const cadences = this.specs.map((spec) =>
      spec.periodMs !== undefined
        ? Math.max(1, Math.round(spec.periodMs / this.intervalMs))
        : this.trayOnly
          ? spec.idleCadence
          : spec.cadence
    );
    const sampled = this.specs.map(
      (_, i) => land || this.tick % (cadences[i] as number) === 0
    );
    const ticksPerSecond = 1000 / this.intervalMs;
    const values = this.specs.map((spec, i) => {
      const st = this.state[i] as { v: number; r: () => number };
      if (spec.noise > 0) st.v = walkStep(st.v, st.r, spec);
      if (land) st.v = spec.base;
      if (!sampled[i] || this.failing.has(i) || spec.sumOf) return null;
      const scaled =
        spec.perTick === undefined
          ? st.v
          : st.v - spec.perTick * (1 - ticksPerSecond);
      return round(clampWalk(scaled, spec));
    });
    this.specs.forEach((spec, i) => {
      if (!spec.sumOf || !sampled[i]) return;
      const parts = spec.sumOf.map((p) => values[p] ?? null);
      values[i] = parts.some((v) => v === null)
        ? null
        : round(parts.reduce<number>((s, v) => s + (v ?? 0), 0));
    });
    values.forEach((v, i) => {
      if (v === null) return;
      this.last[i] = v;
      this.lastTick[i] = this.tick;
      this.holdTicks[i] =
        ((cadences[i] as number) * HOLD_FACTOR.num) / HOLD_FACTOR.den;
    });
    const held = this.last.map((v, i) =>
      this.tick - (this.lastTick[i] as number) > (this.holdTicks[i] as number)
        ? null
        : v
    );
    this.tick++;
    return { values, held, cadences };
  }

  /**
   * `n` rows of history ending on the sample values. The first `trayOnly`
   * of them were taken with only the tray open.
   */
  backfill(n: number, trayOnly = 0): GeneratedRow[] {
    const rows: GeneratedRow[] = [];
    for (let i = 0; i < n; i++) {
      this.trayOnly = i < trayOnly;
      rows.push(this.next(i === n - 1));
    }
    this.trayOnly = false;
    return rows;
  }

  /**
   * The top processes, then quiet background and idle ones (800 in all),
   * with a little per-tick jitter.
   */
  processes(): LiveProcess[] {
    const r = rng(7 + this.tick);
    return [...PROCESSES, ...BACKGROUND_PROCESSES, ...IDLE_PROCESSES].map(
      (p) => ({
        ...p,
        cpu_pct:
          p.cpu_pct === null
            ? null
            : round(Math.max(0, p.cpu_pct * (0.85 + r() * 0.3))),
      })
    );
  }
}

function round(v: number): number {
  return Math.round(v * 1000) / 1000;
}
