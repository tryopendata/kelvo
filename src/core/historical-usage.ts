import {
  type ByteUnits,
  formatBytes,
  formatPercent,
  formatSpan,
  formatWattsFine,
} from "@core/format";
import type {
  HistoricalUsageFigures,
  HistoricalUsageMetric,
  UsageResourceCoverage,
} from "@core/generated/bindings";

export const HISTORICAL_COLUMNS: readonly HistoricalUsageMetric[] = [
  "cpu",
  "gpu",
  "memory",
  "energy",
  "network",
];
export const HISTORICAL_LABELS: Record<HistoricalUsageMetric, string> = {
  cpu: "Avg CPU",
  gpu: "Avg GPU",
  memory: "Peak RAM",
  energy: "CPU power est.",
  network: "Network",
};
export function historicalValue(
  figures: HistoricalUsageFigures,
  metric: HistoricalUsageMetric
): number | null {
  switch (metric) {
    case "cpu":
      return figures.cpu_avg_pct;
    case "gpu":
      return figures.gpu_avg_pct;
    case "memory":
      return figures.mem_peak_bytes;
    case "energy":
      return figures.cpu_power_w;
    case "network":
      return figures.network_rx_bytes === null ||
        figures.network_tx_bytes === null
        ? null
        : figures.network_rx_bytes + figures.network_tx_bytes;
  }
}
export function historicalFigure(
  value: number | null,
  metric: HistoricalUsageMetric,
  units: ByteUnits
): string {
  if (metric === "cpu" || metric === "gpu")
    return formatPercent(value, { decimals: 1 });
  if (metric === "energy") return formatWattsFine(value);
  return formatBytes(value, { units });
}
export function historicalDelta(
  current: HistoricalUsageFigures,
  previous: HistoricalUsageFigures | undefined,
  metric: HistoricalUsageMetric,
  units: ByteUnits
) {
  const a = historicalValue(current, metric);
  const b = previous ? historicalValue(previous, metric) : null;
  if (a === null || b === null) return "Difference unavailable";
  const difference = a - b;
  const number =
    metric === "cpu" || metric === "gpu"
      ? `${Math.abs(difference).toFixed(1)} pp`
      : historicalFigure(Math.abs(difference), metric, units);
  return `${difference < 0 ? "−" : difference > 0 ? "+" : ""}${number}`;
}
export function historicalCoverage(
  coverage: UsageResourceCoverage | undefined,
  spanMs: number
): string {
  if (!coverage) return "No coverage available for this interval";
  if (!coverage.supported) return "Not available on this host";
  const time = `${formatSpan(coverage.measured_ms)} of ${formatSpan(spanMs)} recorded`;
  return `${time}${coverage.complete ? "" : "; incomplete observation"}`;
}
