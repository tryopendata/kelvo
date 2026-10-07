/**
 * Popover module cards (plan 4.3) fed by the live store.
 * Each card reads its own module selector, so a frame that only moves CPU
 * values re-renders the CPU cards and nothing else.
 */
import { ratio, stackRemainder } from "@core/chart-math";
import { cpuPowerNote } from "@core/cpu-power-source";
import {
  bytesParts,
  formatBytes,
  formatGhz,
  formatHoursMinutes,
  formatPercent,
  formatWatts,
  MISSING,
  marketingGb,
  rateParts,
} from "@core/format";
import { METRIC_CODES } from "@core/generated/bindings";
import { windowLabel, windowWords } from "@core/live-window";
import { useHostRecord } from "~/hooks/use-host-record";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useReadFailure } from "~/hooks/use-read-failure";
import { useScaledWindow } from "~/hooks/use-scaled-window";
import { useTransport } from "~/lib/transport-context";
import { useHost } from "~/stores/host-store";
import {
  useBattery,
  useCpu,
  useCpuCores,
  useGpu,
  useMemory,
  usePower,
  usePowerSource,
  usePrimaryIface,
  useSeriesWindow,
} from "~/stores/live-selectors";
import { useSettings } from "~/stores/settings-store";
import { CoreTiles } from "~/widgets/core-tiles";
import { InlineBar } from "~/widgets/inline-bar";
import { MirrorBars } from "~/widgets/mirror-bars";
import { ModuleCard } from "~/widgets/module-card";
import { StackBar } from "~/widgets/stack-bar";
import { StatGrid } from "~/widgets/stat-grid";
import { StreamArea } from "~/widgets/stream-area";

/** Opens a card's module page in the dashboard window. */
function useOpen() {
  const transport = useTransport();
  return (href: string) => void transport.openDashboard(href);
}

/** The charts' window at 1 s; slower intervals keep the 60 samples (plan 4.15). */
const WINDOW_MS = 60_000;
const NET_WINDOW_MS = 48_000;

export function LiveCpuCard() {
  const open = useOpen();
  const cpu = useCpu();
  const windowMs = useScaledWindow(WINDOW_MS);
  const total = useSeriesWindow("cpu.total", windowMs);
  const ceiling = useNiceCeiling(total.values, total.tEndMs, 10, windowMs);
  const notice = useReadFailure("cpu.total");
  return (
    <ModuleCard
      accent="cpu"
      title="CPU"
      href="/dashboard/cpu"
      onOpen={open}
      value={formatPercent(cpu.total)}
      notice={notice}
    >
      <StreamArea
        series={[{ key: "cpu.total", values: total.values, step: 1 }]}
        tEndMs={total.tEndMs}
        intervalMs={total.intervalMs}
        yMax={ceiling}
        accent="cpu"
        height={52}
        ariaLabel={`CPU, last ${windowWords(windowMs)}, ${formatPercent(cpu.total)}`}
        ceilingLabel={`${ceiling}%`}
        windowLabel={windowLabel(windowMs)}
        gridlines={[ceiling / 2]}
      />
      <div className="flex flex-col gap-[5px] pt-1.5">
        <InlineBar
          label="User"
          value={formatPercent(cpu.user, { decimals: 1 })}
          fraction={ratio(cpu.user, 100)}
          layout="row"
        />
        <InlineBar
          label="System"
          value={formatPercent(cpu.system, { decimals: 1 })}
          fraction={ratio(cpu.system, 100)}
          rampStep={2}
          layout="row"
        />
      </div>
    </ModuleCard>
  );
}

export function LiveCoresCard() {
  const open = useOpen();
  const cores = useCpuCores();
  const cpu = useCpu();
  const host = useHostRecord();
  const clusters = (host?.info.cpu_topology ?? []).map((c) => ({
    id: c.name,
    name: c.kind === "efficiency" ? "E-cluster" : "P-cluster",
    freq: formatGhz(c.kind === "efficiency" ? cpu.eFreqHz : cpu.pFreqHz),
    cores: c.cores.map((id) => ({ id, load: cores[id] ?? null })),
  }));
  return (
    <ModuleCard
      accent="cpu"
      title="Cores"
      href="/dashboard/cpu"
      onOpen={open}
      subtitle="% load"
      subtitleStyle="label"
    >
      <CoreTiles clusters={clusters} />
    </ModuleCard>
  );
}

export function LiveMemoryCard() {
  const open = useOpen();
  const mem = useMemory();
  const host = useHostRecord();
  const notice = useReadFailure("mem.used");
  const total = host?.info.mem_total_bytes ?? null;
  // Used follows the GB/GiB setting; the total is the marketing size (plan 4.9).
  const bytes = useSettings((s) => s.units.memory) === "binary" ? "GiB" : "GB";
  const frac = (v: number | null) => ratio(v, total);
  const codes = METRIC_CODES["mem.pressure_level"];
  const level =
    mem.pressureLevel === codes.critical
      ? "critical"
      : mem.pressureLevel === codes.warn
        ? "warn"
        : "normal";
  return (
    <ModuleCard
      accent="mem"
      title="Memory"
      href="/dashboard/memory"
      onOpen={open}
      value={
        mem.used === null
          ? MISSING
          : bytesParts(mem.used, { units: bytes, unit: bytes }).value
      }
      unit={total === null ? undefined : ` / ${marketingGb(total)} GB`}
      notice={notice}
    >
      <InlineBar
        label={`Pressure · ${level}`}
        value={formatPercent(mem.pressure)}
        fraction={ratio(mem.pressure, 100)}
        layout="wide"
      />
      <StackBar
        showLegend
        legendColumns={2}
        segments={[
          seg("wired", "Wired", mem.wired, frac(mem.wired), 1),
          seg("app", "App", mem.app, frac(mem.app), 2),
          seg(
            "compressed",
            "Compressed",
            mem.compressed,
            frac(mem.compressed),
            "hatch"
          ),
          seg("cached", "Cached files", mem.cached, frac(mem.cached), 4),
          seg("free", "Free", mem.free, frac(mem.free), "track"),
        ]}
        extraLegend={[{ label: "Swap", value: formatBytes(mem.swapUsed) }]}
      />
    </ModuleCard>
  );
}

function seg(
  key: string,
  label: string,
  bytes: number | null,
  fraction: number | null,
  step: 1 | 2 | 3 | 4 | "hatch" | "track"
) {
  return { key, label, value: formatBytes(bytes), fraction, step };
}

/**
 * GPU card. The third cell would be the GPU core count, but `HostInfo` has
 * no GPU core count yet, so the card shows the renderer utilization it can measure.
 */
export function LiveGpuCard() {
  const open = useOpen();
  const gpu = useGpu();
  const windowMs = useScaledWindow(WINDOW_MS);
  const util = useSeriesWindow("gpu.util", windowMs);
  const notice = useReadFailure("gpu.util");
  return (
    <ModuleCard
      accent="gpu"
      title="GPU"
      href="/dashboard/gpu"
      onOpen={open}
      value={formatPercent(gpu.util)}
      notice={notice}
    >
      <StreamArea
        series={[{ key: "gpu.util", values: util.values, step: 1 }]}
        tEndMs={util.tEndMs}
        intervalMs={util.intervalMs}
        yMax={100}
        accent="gpu"
        height={40}
        ariaLabel={`GPU, last ${windowWords(windowMs)}, ${formatPercent(gpu.util)}`}
      />
      <StatGrid
        items={[
          { label: "Freq", value: formatGhz(gpu.freqHz) },
          { label: "Power", value: formatWatts(gpu.powerW) },
          { label: "Render", value: formatPercent(gpu.render) },
        ]}
      />
    </ModuleCard>
  );
}

/**
 * Power card: system watts, a stacked bar of CPU, GPU, ANE (hatched) and
 * DRAM with the rest of the system as track, and the four values with keys.
 */
export function LivePowerCard() {
  const open = useOpen();
  const p = usePower();
  const notice = useReadFailure("power.system");
  const cpuNote = cpuPowerNote(p.cpuSource);
  // The rest of the system is what the named rails leave.
  const rest = stackRemainder(p.system, [p.cpu, p.gpu, p.ane, p.dram]);
  const frac = (v: number | null) => ratio(v, p.system);
  return (
    <ModuleCard
      accent="power"
      title="Power"
      href="/dashboard/power"
      onOpen={open}
      value={formatWatts(p.system)}
      unit=" system"
      notice={notice}
    >
      <StackBar
        segments={[
          seg2("cpu", "CPU", p.cpu, frac(p.cpu), 1),
          seg2("gpu", "GPU", p.gpu, frac(p.gpu), 2),
          seg2("ane", "ANE", p.ane, frac(p.ane), "hatch"),
          seg2("dram", "DRAM", p.dram, frac(p.dram), 3),
          seg2("rest", "Rest of system", rest, frac(rest), "track"),
        ]}
      />
      <StatGrid
        items={[
          { label: "CPU", value: formatWatts(p.cpu), step: 1 },
          { label: "GPU", value: formatWatts(p.gpu), step: 2 },
          {
            label: "ANE",
            value: formatWatts(p.ane),
            step: "hatch",
            muted: !p.ane,
          },
          { label: "DRAM", value: formatWatts(p.dram), step: 3 },
        ]}
      />
      {cpuNote && (
        <p className="m-0 font-normal text-[11px] text-muted-foreground">
          {cpuNote}
        </p>
      )}
    </ModuleCard>
  );
}

function seg2(
  key: string,
  label: string,
  watts: number | null,
  fraction: number | null,
  step: 1 | 2 | 3 | "hatch" | "track"
) {
  return { key, label, value: formatWatts(watts), fraction, step };
}

/**
 * Network card: the totals over the reported interfaces (`net.rx_total`,
 * D-092), so the subtitle says so and names the interface carrying the
 * default route ("All interfaces · en0"; "All interfaces" on a full-tunnel
 * VPN). The figures are every interface's, not one interface's ("Wi‑Fi ·
 * en0"), and the interface kind is not reported yet.
 */
export function LiveNetworkCard() {
  const open = useOpen();
  const iface = usePrimaryIface();
  const bits = useSettings((s) => s.units.network) === "bits_per_sec";
  const rxKey = "net.rx_total";
  const txKey = "net.tx_total";
  const windowMs = useScaledWindow(NET_WINDOW_MS);
  const rx = useSeriesWindow(rxKey, windowMs);
  const tx = useSeriesWindow(txKey, windowMs);
  const rxNow = useHost((s) => s.held[rxKey] ?? null);
  const txNow = useHost((s) => s.held[txKey] ?? null);
  const rate = (bps: number | null) => {
    if (bps === null) return { value: "–" };
    const q = rateParts(bps, { units: bits ? "Mbps" : "MBps" });
    return { value: q.value, unit: q.unit };
  };
  return (
    <ModuleCard
      accent="net"
      title="Network"
      href="/dashboard/network"
      onOpen={open}
      subtitle={iface === null ? "All interfaces" : `All interfaces · ${iface}`}
      subtitleStyle="text"
    >
      <StatGrid
        items={[
          { label: "↑ Upload", ...rate(txNow) },
          { label: "↓ Download", ...rate(rxNow) },
        ]}
      />
      <MirrorBars
        up={tx.values}
        down={rx.values}
        intervalMs={rx.intervalMs}
        tEndMs={rx.tEndMs}
        accent="net"
        ariaLabel={`Upload above the line, download below, last ${windowWords(windowMs)}`}
        upHeight={18}
        downHeight={30}
      />
    </ModuleCard>
  );
}

/** Battery card: charge bar, time remaining, health and cycles. */
export function LiveBatteryCard() {
  const open = useOpen();
  const b = useBattery();
  const source = usePowerSource();
  const notice = useReadFailure("battery.charge");
  const remaining =
    b.timeRemainingMin === null
      ? source === "adapter" || source === "charging"
        ? "On power"
        : "–"
      : formatHoursMinutes(b.timeRemainingMin * 60_000);
  return (
    <ModuleCard
      accent="battery"
      title="Battery"
      href="/dashboard/battery"
      onOpen={open}
      value={formatPercent(b.charge)}
      notice={notice}
    >
      {/* The charge is the headline figure; the bar repeats it visually. */}
      <span
        aria-hidden
        className="block h-1.5 overflow-hidden rounded-full bg-track"
      >
        <span
          className="block h-full origin-left rounded-full bg-battery"
          style={{ transform: `scaleX(${ratio(b.charge, 100)})` }}
        />
      </span>
      <StatGrid
        items={[
          { label: "Remaining", value: remaining },
          { label: "Health", value: formatPercent(b.health) },
          {
            label: "Cycles",
            value: b.cycles === null ? "–" : String(Math.round(b.cycles)),
          },
        ]}
      />
    </ModuleCard>
  );
}
