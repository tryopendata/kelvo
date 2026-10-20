import { clamp01 } from "./numeric";

export const HEATMAP_ALPHA_MIN = 0.06;
export const HEATMAP_ALPHA_RANGE = 0.89;

/**
 * Cell alpha for the accent-tinted heatmaps (per-core load, the 30-day
 * heatmap, core tiles): `0.06 + clamp(v / vmax, 0, 1) * 0.89`, so it tops out
 * at 0.95. `vmax` is per metric: 100 for per-core load, 80 for the
 * 30-day CPU heatmap. `vmin` lifts the floor for scales that do not start
 * at 0 (20 to 110 °C for temperature): `t = (v - vmin) / (vmax - vmin)`.
 * A missing value is `null`: the cell draws the hatched "no samples"
 * pattern, not the faintest tint.
 */
export function heatmapAlpha(
  value: number | null | undefined,
  vmax: number,
  vmin = 0
): number | null {
  if (value == null || !Number.isFinite(value)) return null;
  const t = vmax > vmin ? clamp01((value - vmin) / (vmax - vmin)) : 0;
  return HEATMAP_ALPHA_MIN + t * HEATMAP_ALPHA_RANGE;
}
