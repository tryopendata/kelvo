import { fixed, isPresent, type MaybeNumber, MISSING } from "./number";

const significantFormatters = new Map<number, Intl.NumberFormat>();

export interface PercentOptions {
  /** Default 0 ("18%"). Breakdowns use 1 ("12.4%", "82.0%"). */
  decimals?: number;
  /** Adaptive precision for small percentages, e.g. 0.0173% and 58.6%. */
  significantDigits?: number;
}

/**
 * `value` is already in percent (0 to 100, or above 100 for per-process CPU,
 * "412%"), matching the catalog's `%` unit. No space before the sign.
 */
export function formatPercent(
  value: MaybeNumber,
  { decimals = 0, significantDigits }: PercentOptions = {}
): string {
  if (!isPresent(value)) return MISSING;
  if (significantDigits !== undefined) {
    let formatter = significantFormatters.get(significantDigits);
    if (!formatter) {
      formatter = new Intl.NumberFormat("en-US", {
        maximumSignificantDigits: significantDigits,
      });
      significantFormatters.set(significantDigits, formatter);
    }
    return `${formatter.format(value)}%`;
  }
  return `${fixed(value, decimals)}%`;
}
