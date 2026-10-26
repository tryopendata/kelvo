import { formatBytes, formatPercent, MISSING } from "@core/format";
import type { MetricStat, SeriesStats } from "@core/generated/bindings";
import { formatMetric, type LaneUnits } from "./format";
import type { LaneId } from "./lanes";

export const SUMMARY_METRICS = [
  "cpu.total",
  "gpu.util",
  "mem.used",
  "power.system",
  "thermal.hottest",
  "net.rx_total",
  "net.tx_total",
];
export interface LaneSummary {
  value: string;
  detail: string;
  coverage: string;
}

/** Formats Rust-owned statistics; never recomputes a mean from plotted buckets. */
export function laneSummary(
  id: LaneId,
  stats: SeriesStats | undefined,
  units: LaneUnits,
  error = false
): LaneSummary {
  const find = (metric: string) =>
    stats?.metrics.find((item) => item.metric === metric);
  const pending: LaneSummary = {
    value: MISSING,
    detail: error ? "Summary unavailable" : "Loading range summary…",
    coverage: "",
  };
  if (!stats) return pending;
  if (id === "network") {
    const rx = find("net.rx_total"),
      tx = find("net.tx_total");
    const total =
      rx?.integral == null || tx?.integral == null
        ? null
        : rx.integral + tx.integral;
    return {
      value: formatBytes(total, { units: units.bytes ?? "GB" }),
      detail: "Total transferred",
      coverage: `RX ${coverage(stats, rx)} · TX ${coverage(stats, tx)}`,
    };
  }
  const metric = {
    cpu: "cpu.total",
    gpu: "gpu.util",
    memory: "mem.used",
    power: "power.system",
    temp: "thermal.hottest",
  }[id];
  const stat = find(metric);
  const peak = id === "memory" || id === "temp";
  const value = peak ? stat?.max : stat?.avg;
  return {
    value:
      id === "memory"
        ? formatBytes(value, { units: units.bytes ?? "GB" })
        : formatMetric(metric, value, units),
    detail: {
      cpu: `Average · peak ${formatPercent(stat?.max)}`,
      gpu: `Average · peak ${formatPercent(stat?.max)}`,
      power: "Average system power",
      memory: "Peak memory used",
      temp: "Maximum temperature",
    }[id],
    coverage: coverage(stats, stat),
  };
}

function coverage(stats: SeriesStats, stat: MetricStat | undefined) {
  if (!stat) return "Coverage unavailable";
  const duration = stats.to_ms - stats.from_ms;
  return duration > 0
    ? `${Math.min(100, Math.round((stat.measured_ms / duration) * 100))}% measured`
    : "No measured time";
}
