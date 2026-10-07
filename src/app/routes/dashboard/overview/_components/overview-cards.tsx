import { windowLabel, windowWords } from "@core/live-window";
import { useScaledWindow } from "~/hooks/use-scaled-window";
/**
 * The six Overview cards (plan 4.5), each subscribed to its
 * own slice so one module's tick does not re-render the others.
 */

import { formatPercent } from "@core/format";
import type { LiveProcess } from "@core/generated/bindings";
import { useNavigate } from "react-router";
import { useShallow } from "zustand/react/shallow";
import { useGpuMaxMhz, useHostRecord } from "~/hooks/use-host-record";
import { useProcessGpu, useProcessNetwork } from "~/hooks/use-process-interest";
import { useReadFailure } from "~/hooks/use-read-failure";
import { useHost } from "~/stores/host-store";
import {
  useCpu,
  useDisk,
  useGpu,
  useMemory,
  useNetwork,
  usePower,
  usePowerSource,
  usePrimaryIface,
  useSensors,
  useSeriesWindow,
} from "~/stores/live-selectors";
import { useSettings } from "~/stores/settings-store";
import type { Corner } from "~/widgets/lib/accent";
import { MetricCard } from "~/widgets/metric-card";
import { useMax24h } from "../_hooks/use-overview-history";
import {
  type ClusterInput,
  coresLabel,
  cpuCard,
  diskCard,
  gpuCard,
  memoryCard,
  networkCard,
  powerCard,
} from "../_lib/card-props";
import { scaleFrom } from "../_lib/history";
import { ifaceRates, linkRate, volumeCapacity } from "../_lib/selectors";

const NO_PROCESSES: readonly LiveProcess[] = [];

function useProcesses(): readonly LiveProcess[] {
  return useHost((s) => s.processes?.rows ?? NO_PROCESSES);
}

function useRateUnits() {
  return useSettings((s) => s.units.network) === "bits_per_sec"
    ? ("Mbps" as const)
    : ("MBps" as const);
}

interface CardProps {
  origin: Corner;
}

function useOpen() {
  const navigate = useNavigate();
  return (href: string) => navigate(href);
}

export function OverviewCpuCard({ origin }: CardProps) {
  const cpu = useCpu();
  const host = useHostRecord();
  const topology = host?.info.cpu_topology ?? [];
  const p = topology.find((c) => c.kind === "performance");
  const e = topology.find((c) => c.kind === "efficiency");
  const freq = useHost(
    useShallow((s) => ({
      p: p ? (s.held[`cpu.cluster.freq{cluster=${p.name}}`] ?? null) : null,
      e: e ? (s.held[`cpu.cluster.freq{cluster=${e.name}}`] ?? null) : null,
    }))
  );
  const maxHz = (mhz: readonly number[] | undefined) => {
    const top = mhz?.[mhz.length - 1];
    return top === undefined ? null : top * 1e6;
  };
  const clusters: [ClusterInput, ClusterInput] = [
    { label: "P-cluster", freqHz: freq.p, maxHz: maxHz(p?.dvfs_mhz) },
    { label: "E-cluster", freqHz: freq.e, maxHz: maxHz(e?.dvfs_mhz) },
  ];
  const props = cpuCard({
    ...cpu,
    coresLabel: coresLabel(topology),
    coreCount: topology.reduce((n, c) => n + c.cores.length, 0),
    clusters,
    processes: useProcesses(),
  });
  return (
    <MetricCard
      {...props}
      origin={origin}
      notice={useReadFailure("cpu.total")}
      href="/dashboard/cpu"
      onOpen={useOpen()}
    />
  );
}

/**
 * The GPU card: top 5 processes by GPU share with per-process GPU time
 * (D-085), else the 60 s utilization chart. Two components, so the list
 * version neither reads the ring buffer nor re-renders on its version.
 */
export function OverviewGpuCard({ origin }: CardProps) {
  return useProcessGpu() ? (
    <GpuProcessesCard origin={origin} />
  ) : (
    <GpuChartCard origin={origin} />
  );
}

function useGpuCardInput() {
  const gpu = useGpu();
  const maxMhz = useGpuMaxMhz();
  const maxFreqHz = maxMhz === null ? null : maxMhz * 1e6;
  const powerMax = useMax24h("power", "power.gpu");
  return {
    ...gpu,
    maxFreqHz,
    powerScaleW: scaleFrom(powerMax, 5),
    subtitle: "",
  };
}

function GpuChartCard({ origin }: CardProps) {
  const input = useGpuCardInput();
  const windowMs = useScaledWindow(60_000);
  const util = useSeriesWindow("gpu.util", windowMs);
  const props = gpuCard({
    ...input,
    body: {
      stream: {
        series: [{ key: "gpu.util", values: util.values, step: 1 }],
        tEndMs: util.tEndMs,
        intervalMs: util.intervalMs,
        yMax: 100,
        accent: "gpu",
        height: 98,
        ariaLabel: `GPU, last ${windowWords(windowMs)}, ${formatPercent(input.util)}`,
        windowLabel: windowLabel(windowMs),
      },
    },
  });
  return (
    <MetricCard
      {...props}
      origin={origin}
      notice={useReadFailure("gpu.util")}
      href="/dashboard/gpu"
      onOpen={useOpen()}
    />
  );
}

function GpuProcessesCard({ origin }: CardProps) {
  const props = gpuCard({
    ...useGpuCardInput(),
    body: { processes: useProcesses() },
  });
  return (
    <MetricCard
      {...props}
      origin={origin}
      notice={useReadFailure("gpu.util")}
      href="/dashboard/gpu"
      onOpen={useOpen()}
    />
  );
}

export function OverviewMemoryCard({ origin }: CardProps) {
  const mem = useMemory();
  const host = useHostRecord();
  const binary = useSettings((s) => s.units.memory) === "binary";
  const props = memoryCard({
    ...mem,
    totalBytes: host?.info.mem_total_bytes ?? null,
    binary,
    processes: useProcesses(),
  });
  return (
    <MetricCard
      {...props}
      origin={origin}
      notice={useReadFailure("mem.used")}
      href="/dashboard/memory"
      onOpen={useOpen()}
    />
  );
}

export function OverviewPowerCard({ origin }: CardProps) {
  const p = usePower();
  const sensors = useSensors();
  const powerSource = usePowerSource();
  const fahrenheit = useSettings((s) => s.units.temperature) === "fahrenheit";
  // A layout with sensors but no fan series is a Mac without fans.
  const hasSensorLayout = useHost(
    (s) =>
      s.layoutNo !== null &&
      (s.layouts[s.layoutNo]?.byMetric.has("thermal.hottest") ?? false)
  );
  const props = powerCard({
    system: p.system,
    cpu: p.cpu,
    gpu: p.gpu,
    dram: p.dram,
    hottestC: sensors.hottest,
    fanRpm: sensors.fanRpm,
    fanMax: sensors.fanMax,
    passive: hasSensorLayout && sensors.fanCount === 0,
    powerSource,
    temperature: fahrenheit ? "F" : "C",
    processes: useProcesses(),
  });
  return (
    <MetricCard
      {...props}
      origin={origin}
      notice={useReadFailure("power.system")}
      href="/dashboard/power"
      onOpen={useOpen()}
    />
  );
}

export function OverviewNetworkCard({ origin }: CardProps) {
  const primary = usePrimaryIface();
  const net = useNetwork();
  const rates = useHost(useShallow(ifaceRates));
  const link = useHost((s) => linkRate(s, primary));
  const rate = useRateUnits();
  const perProcess = useProcessNetwork();
  // Without per-process network a process batch does not touch this card.
  const processes = useHost((s) =>
    perProcess ? (s.processes?.rows ?? NO_PROCESSES) : null
  );
  const names = [
    ...new Set(Object.keys(rates).map((k) => k.slice(k.indexOf("|") + 1))),
  ];
  const interfaces = names.map((iface) => ({
    iface,
    rx: rates[`rx|${iface}`] ?? null,
    tx: rates[`tx|${iface}`] ?? null,
  }));
  const props = networkCard({
    primary: interfaces.find((i) => i.iface === primary) ?? null,
    linkBps: link,
    total: { rx: net.rx, tx: net.tx },
    interfaces,
    rate,
    processes,
  });
  return (
    <MetricCard
      {...props}
      origin={origin}
      href="/dashboard/network"
      onOpen={useOpen()}
    />
  );
}

export function OverviewDiskCard({ origin }: CardProps) {
  const disk = useDisk();
  const boot = useHostRecord()?.info.boot_mounts?.[0] ?? null;
  const vol = useHost(useShallow((s) => volumeCapacity(s, boot)));
  const readMax = useMax24h("disk", "disk.read_total");
  const writeMax = useMax24h("disk", "disk.write_total");
  const props = diskCard({
    read: disk.read,
    write: disk.write,
    totalBytes: vol.total,
    freeBytes: vol.free,
    usedBytes: vol.used,
    readScale: scaleFrom(readMax, 500e6),
    writeScale: scaleFrom(writeMax, 500e6),
    rate: useRateUnits(),
    processes: useProcesses(),
  });
  return (
    <MetricCard
      {...props}
      origin={origin}
      href="/dashboard/disk"
      onOpen={useOpen()}
    />
  );
}
