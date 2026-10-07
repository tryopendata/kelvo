import type { LiveProcess } from "@core/generated/bindings";

/**
 * Process rows with measured GPU time: a share on the row (not the baseline
 * sample, D-085) and above zero.
 */
export function measuredGpu(rows: readonly LiveProcess[]): LiveProcess[] {
  return rows.filter((p) => p.gpu_pct !== null && p.gpu_pct > 0);
}
