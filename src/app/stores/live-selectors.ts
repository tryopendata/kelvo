/**
 * Per-module selectors over a host's live state. Each returns a flat object
 * of primitives so `useShallow` can compare it: a frame that only moves CPU
 * values leaves the Memory selector's result shallow-equal, and the Memory
 * card does not re-render (the render-count test pins this).
 *
 * Values are raw units from the catalog (percent, Hz, bytes, bytes/s, W,
 * °C). `null` means no current value: a gap or a missing series, never 0.
 */
import type { PowerSource } from "@core/generated/bindings";
import { type SeriesWindow, seriesWindow } from "@core/live-state";
import { useShallow } from "zustand/react/shallow";
import { type HostLiveState, useHost, useHostStore } from "./host-store";

type S = HostLiveState;

const held = (s: S, key: string): number | null => s.held[key] ?? null;

/** Keys of `metric` in the current layout. */
function keysOf(s: S, metric: string): readonly string[] {
  if (s.layoutNo === null) return [];
  return s.layouts[s.layoutNo]?.byMetric.get(metric) ?? [];
}

/** Max over every label set of `metric`; null when none has a value. */
function max(s: S, metric: string): number | null {
  let out: number | null = null;
  for (const k of keysOf(s, metric)) {
    const v = held(s, k);
    if (v !== null && (out === null || v > out)) out = v;
  }
  return out;
}

/** `{ labelValue: value }` for one label of `metric`. */
function byLabel(s: S, metric: string): Record<string, number | null> {
  const out: Record<string, number | null> = {};
  for (const k of keysOf(s, metric)) {
    const inner = k.slice(k.indexOf("{") + 1, -1);
    out[inner.slice(inner.indexOf("=") + 1)] = held(s, k);
  }
  return out;
}

export const selectCpu = (s: S) => ({
  total: held(s, "cpu.total"),
  user: held(s, "cpu.user"),
  system: held(s, "cpu.system"),
  pFreqHz: held(s, "cpu.cluster.freq{cluster=P0}"),
  eFreqHz: held(s, "cpu.cluster.freq{cluster=E0}"),
});

/** Per-core load keyed by core label ("P0", "E3"). */
export const selectCpuCores = (s: S) => byLabel(s, "cpu.load");

export const selectGpu = (s: S) => ({
  util: held(s, "gpu.util"),
  render: held(s, "gpu.render"),
  tiler: held(s, "gpu.tiler"),
  freqHz: held(s, "gpu.freq"),
  powerW: held(s, "power.gpu"),
});

export const selectMemory = (s: S) => ({
  used: held(s, "mem.used"),
  app: held(s, "mem.app"),
  wired: held(s, "mem.wired"),
  compressed: held(s, "mem.compressed"),
  cached: held(s, "mem.cached"),
  free: held(s, "mem.free"),
  pressure: held(s, "mem.pressure"),
  pressureLevel: held(s, "mem.pressure_level"),
  swapUsed: held(s, "mem.swap_used"),
});

export const selectPower = (s: S) => ({
  system: held(s, "power.system"),
  cpu: held(s, "power.cpu"),
  gpu: held(s, "power.gpu"),
  ane: held(s, "power.ane"),
  dram: held(s, "power.dram"),
  package: held(s, "power.package"),
  /** `power.cpu_source` (D-065); null when absent (PMP) or not current. */
  cpuSource: held(s, "power.cpu_source"),
});

export const selectSensors = (s: S) => ({
  hottest: held(s, "thermal.hottest"),
  cpu: held(s, "thermal.cpu"),
  gpu: held(s, "thermal.gpu"),
  thermalState: held(s, "thermal.state"),
  fanRpm: max(s, "fan.rpm"),
  fanMax: max(s, "fan.max"),
  fanCount: keysOf(s, "fan.rpm").length,
});

/**
 * `net.rx_total` / `net.tx_total`: Rust sums the reported interfaces and makes
 * a total a gap when any of them is (D-092), so this never adds parts.
 */
export const selectNetwork = (s: S) => ({
  rx: held(s, "net.rx_total"),
  tx: held(s, "net.tx_total"),
  linkRate: max(s, "net.link_rate"),
});

/**
 * The reported interface carrying the default route (`LiveStatus.primary_iface`,
 * D-092); null on a VPN route, with no route, or before the first status.
 */
export const selectPrimaryIface = (s: S): string | null =>
  s.status?.primary_iface ?? null;

/** `disk.read_total` / `disk.write_total`, gated like the network totals. */
export const selectDisk = (s: S) => ({
  read: held(s, "disk.read_total"),
  write: held(s, "disk.write_total"),
});

export const selectBattery = (s: S) => ({
  charge: held(s, "battery.charge"),
  charging: held(s, "battery.charging"),
  external: held(s, "battery.external"),
  timeRemainingMin: held(s, "battery.time_remaining"),
  health: held(s, "battery.health"),
  cycles: held(s, "battery.cycles"),
});

/** Sampling state for pills and footers. */
export const selectSampling = (s: S) => ({
  intervalMs: s.status?.interval_ms ?? null,
  /** How often this window gets frames: 2 s or more in Performance mode. */
  framePeriodMs: s.status?.frame_period_ms ?? null,
  paused: s.status?.paused ?? false,
  onBattery: s.status?.on_battery ?? false,
  performance: s.status?.performance ?? "off",
  stale: s.stale,
  connection: s.connection,
});

export const selectSelfCpu = (s: S) => held(s, "self.cpu");

/** Battery, adapter or charging (`LiveStatus.power_source`, D-092); null before the first status. */
export const selectPowerSource = (s: S): PowerSource | null =>
  s.status?.power_source ?? null;

/** Series in the current layout, for the history size projection; null before one. */
export const selectSeriesCount = (s: S): number | null =>
  s.layoutNo === null ? null : (s.layouts[s.layoutNo]?.series.length ?? null);

export const useCpu = () => useHost(useShallow(selectCpu));
export const useCpuCores = () => useHost(useShallow(selectCpuCores));
export const useGpu = () => useHost(useShallow(selectGpu));
export const useMemory = () => useHost(useShallow(selectMemory));
export const usePower = () => useHost(useShallow(selectPower));
export const useSensors = () => useHost(useShallow(selectSensors));
export const useNetwork = () => useHost(useShallow(selectNetwork));
export const useDisk = () => useHost(useShallow(selectDisk));
export const useBattery = () => useHost(useShallow(selectBattery));
export const useSampling = () => useHost(useShallow(selectSampling));
export const useSelfCpu = () => useHost(selectSelfCpu);
export const usePrimaryIface = () => useHost(selectPrimaryIface);
export const usePowerSource = () => useHost(selectPowerSource);
export const useSeriesCount = () => useHost(selectSeriesCount);

/**
 * One series over the last `windowMs` on the interval grid, for a live
 * chart. Re-renders when rows are appended (once per tick).
 */
export function useSeriesWindow(key: string, windowMs: number): SeriesWindow {
  const store = useHostStore();
  useHost((s) => s.rowsVersion);
  return seriesWindow(store.getState(), key, windowMs);
}
