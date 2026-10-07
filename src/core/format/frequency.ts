import { fixed, isPresent, type MaybeNumber, MISSING } from "./number";

export interface FrequencyOptions {
  /** Default 1 ("3.2 GHz"); the CPU cluster rings use 2 ("3.20"). */
  decimals?: number;
}

/** Hz as GHz: "3.2 GHz". */
export function formatGhz(
  hz: MaybeNumber,
  { decimals = 1 }: FrequencyOptions = {}
): string {
  if (!isPresent(hz)) return MISSING;
  return `${fixed(hz / 1e9, decimals)} GHz`;
}
