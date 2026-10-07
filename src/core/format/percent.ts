import { fixed, isPresent, type MaybeNumber, MISSING } from "./number";

export interface PercentOptions {
  /** Default 0 ("18%"). Breakdowns use 1 ("12.4%", "82.0%"). */
  decimals?: number;
}

/**
 * `value` is already in percent (0 to 100, or above 100 for per-process CPU,
 * "412%"), matching the catalog's `%` unit. No space before the sign.
 */
export function formatPercent(
  value: MaybeNumber,
  { decimals = 0 }: PercentOptions = {}
): string {
  if (!isPresent(value)) return MISSING;
  return `${fixed(value, decimals)}%`;
}
