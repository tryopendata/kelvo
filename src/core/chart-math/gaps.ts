/** A run of consecutive present samples. `start` indexes the input series. */
export interface Run {
  start: number;
  values: number[];
}

/**
 * Split a series on `null` (and non-finite values) into runs to draw as
 * separate segments. Nothing bridges a gap: no segment, no area, no
 * interpolation across it. Single-sample runs are kept; whether to draw a lone
 * point as a dot is the renderer's call.
 */
export function splitGaps(series: readonly (number | null)[]): Run[] {
  const runs: Run[] = [];
  let current: Run | null = null;
  series.forEach((v, i) => {
    if (v == null || !Number.isFinite(v)) {
      current = null;
      return;
    }
    if (current === null) {
      current = { start: i, values: [] };
      runs.push(current);
    }
    current.values.push(v);
  });
  return runs;
}
