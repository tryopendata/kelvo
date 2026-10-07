import {
  formatGhz,
  formatPercent,
  formatRate,
  formatRpm,
  formatTemperature,
  formatWatts,
  type RateUnits,
  type TemperatureUnits,
} from "@core/format";
import { METRIC_UNITS, type UnitSettings } from "@core/generated/bindings";

type Unit = (typeof METRIC_UNITS)[keyof typeof METRIC_UNITS];

export interface LaneUnits {
  rate: RateUnits;
  temperature: TemperatureUnits;
}

/** The display units from the Units settings; MB/s and °C before they load. */
export function laneUnits(units: UnitSettings | null): LaneUnits {
  return {
    rate: units?.network === "bits_per_sec" ? "Mbps" : "MBps",
    temperature: units?.temperature === "fahrenheit" ? "F" : "C",
  };
}

/** The catalog unit of `metric`; undefined for a metric this build does not know. */
function unitOf(metric: string): Unit | undefined {
  return (METRIC_UNITS as Partial<Record<string, Unit>>)[metric];
}

/** One value of a Timeline metric in its catalog unit, formatted for display. */
export function formatMetric(
  metric: string,
  value: number | null | undefined,
  units: LaneUnits
): string {
  switch (unitOf(metric)) {
    case "watts":
      return formatWatts(value);
    case "celsius":
      return formatTemperature(value, { units: units.temperature });
    case "bytes_per_sec":
      return formatRate(value, { units: units.rate });
    case "rpm":
      return formatRpm(value);
    case "hz":
      return formatGhz(value);
    default:
      return formatPercent(value);
  }
}
