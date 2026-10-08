import type { TrayReadings, TrayUnits } from "@core/tray-layout";
import { useStore } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { SPARK_SAMPLES } from "~/components/tray-preview";
import { useHostRecord } from "~/hooks/use-host-record";
import { useRecent } from "~/hooks/use-recent";
import { type HostLiveState, useHost } from "~/stores/host-store";
import {
  selectBattery,
  selectCpu,
  selectCpuCores,
  selectDisk,
  selectGpu,
  selectMemory,
  selectNetwork,
  selectPower,
  selectSensors,
} from "~/stores/live-selectors";
import { useSettingsStore } from "~/stores/settings-store";

function select(s: HostLiveState, bootMount: string | null) {
  const disk = selectDisk(s);
  const read = disk.read;
  const write = disk.write;
  const used = bootMount
    ? (s.held[`disk.used{vol=${bootMount}}`] ?? null)
    : null;
  const total = bootMount
    ? (s.held[`disk.total{vol=${bootMount}}`] ?? null)
    : null;
  const net = selectNetwork(s);
  return {
    cpu: selectCpu(s).total,
    gpu: selectGpu(s).util,
    mem: selectMemory(s).pressure,
    temp: selectSensors(s).hottest,
    power: selectPower(s).system,
    netUp: net.tx,
    netDown: net.rx,
    diskRate: read === null || write === null ? null : read + write,
    // Rust's `disk_used_pct`: a gap unless the total is a real size.
    diskUsed:
      used === null || total === null || total <= 0
        ? null
        : (used / total) * 100,
    battery: selectBattery(s).charge,
  };
}

/** Per-core load as P-core then E-core clusters, each in core order. */
function clusters(byCore: Record<string, number | null>) {
  const out: Record<string, { n: number; v: number | null }[]> = {};
  for (const [label, v] of Object.entries(byCore)) {
    const kind = label.charAt(0);
    out[kind] = [...(out[kind] ?? []), { n: Number(label.slice(1)), v }];
  }
  return Object.keys(out)
    .sort((a, b) => (a === "P" ? -1 : b === "P" ? 1 : a.localeCompare(b)))
    .map((k) => (out[k] ?? []).sort((a, b) => a.n - b.n).map((c) => c.v));
}

/**
 * What the menu bar would print right now, for the menu bar previews: the
 * same series the Rust tray model reads (`TraySeries`), Disk used from the
 * boot volume, and short CPU and GPU histories for the graph items.
 */
export function useTrayReadings(): TrayReadings {
  const bootMount = useHostRecord()?.info.boot_mounts?.[0] ?? null;
  const values = useHost(useShallow((s) => select(s, bootMount)));
  const cores = useHost(useShallow(selectCpuCores));
  const tick = useHost((s) => s.lastTsMs);
  const cpuHistory = useRecent(values.cpu, tick, SPARK_SAMPLES);
  const gpuHistory = useRecent(values.gpu, tick, SPARK_SAMPLES);
  return { ...values, cpuHistory, gpuHistory, cores: clusters(cores) };
}

/** The Units settings the menu bar formats with (°F and bytes until they arrive). */
export function useTrayUnits(): TrayUnits {
  return useStore(
    useSettingsStore(),
    useShallow((s) => ({
      temperature: s.snapshot?.settings.units.temperature ?? "fahrenheit",
      network: s.snapshot?.settings.units.network ?? "bytes_per_sec",
    }))
  );
}
