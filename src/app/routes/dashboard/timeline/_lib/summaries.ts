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

export interface LaneComparison {
  previous: string;
  delta: string;
  coverage: string;
}
export function laneComparison(
  id: LaneId,
  current: SeriesStats | undefined,
  previous: SeriesStats | undefined,
  units: LaneUnits
): LaneComparison {
  const a = summaryNumber(id, current);
  const b = summaryNumber(id, previous);
  const summary = laneSummary(id, previous, units);
  if (a == null || b == null)
    return {
      previous: summary.value,
      delta: "Difference unavailable",
      coverage: summary.coverage,
    };
  const difference = a - b;
  const abs = Math.abs(difference);
  const formatted = comparisonDifference(id, abs, units);
  return {
    previous: summary.value,
    delta: `${difference < 0 ? "−" : difference > 0 ? "+" : ""}${formatted}`,
    coverage: summary.coverage,
  };
}

function comparisonDifference(id: LaneId, abs: number, units: LaneUnits) {
  return id === "cpu" || id === "gpu"
    ? `${abs.toFixed(1)} pp`
    : id === "memory" || id === "network"
      ? formatBytes(abs, { units: units.bytes ?? "GB" })
      : id === "temp"
        ? `${(abs * (units.temperature === "F" ? 1.8 : 1)).toFixed(1)} °${units.temperature}`
        : formatMetric("power.system", abs, units);
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

function summaryNumber(id: LaneId, stats: SeriesStats | undefined) {
  const get = (metric: string) =>
    stats?.metrics.find((item) => item.metric === metric);
  if (id === "network") {
    const rx = get("net.rx_total")?.integral;
    const tx = get("net.tx_total")?.integral;
    return rx == null || tx == null ? null : rx + tx;
  }
  const metric = {
    cpu: "cpu.total",
    gpu: "gpu.util",
    memory: "mem.used",
    power: "power.system",
    temp: "thermal.hottest",
  }[id];
  const stat = get(metric);
  return id === "memory" || id === "temp" ? stat?.max : stat?.avg;
}
