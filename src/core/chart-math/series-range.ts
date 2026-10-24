/** The line plot and brush both map the first through last grid point. */
export function seriesRange(series: {
  values: Record<string, readonly (number | null)[]>;
  tEndMs: number;
  intervalMs: number;
}) {
  const length = Math.max(
    0,
    ...Object.values(series.values).map((values) => values.length)
  );
  return {
    fromMs: series.tEndMs - Math.max(0, length - 1) * series.intervalMs,
    toMs: series.tEndMs,
  };
}
