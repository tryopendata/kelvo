import { clamp01 } from "./numeric";

export const HEATMAP_ALPHA_MIN = 0.06;
export const HEATMAP_ALPHA_RANGE = 0.89;

/**
 * Cell alpha for the accent-tinted heatmaps (per-core load, the 30-day
 * heatmap, core tiles): `0.06 + clamp(v / vmax, 0, 1) * 0.89`, so it tops out
 * at 0.95. `vmax` is per metric: 100 for per-core load, 80 for the
 * 30-day CPU heatmap. A missing value is `null`: the cell draws the hatched
 * "no samples" pattern, not the faintest tint.
 */
export function heatmapAlpha(
  value: number | null | undefined,
  vmax: number
): number | null {
  if (value == null || !Number.isFinite(value)) return null;
  const t = vmax > 0 ? clamp01(value / vmax) : 0;
  return HEATMAP_ALPHA_MIN + t * HEATMAP_ALPHA_RANGE;
}
