import { useHistorySeries } from "~/hooks/use-history-series";
import {
  MAX_CHART_POINTS,
  useWindowSeries,
  type WindowSeries,
} from "~/hooks/use-window-series";

/** Module charts use the live ring for short rolling windows and stored history otherwise. */
export function useModuleSeries(
  keys: readonly string[],
  windowMs: number,
  options: { brush?: boolean } = {}
): WindowSeries {
  const history = useHistorySeries(keys, windowMs, MAX_CHART_POINTS);
  const live = useWindowSeries(keys, windowMs, {
    ...options,
    enabled: !history.enabled,
  });
  if (!history.enabled) return live;
  return {
    values: history.grid?.values ?? {},
    tEndMs: history.grid?.endMs ?? 0,
    intervalMs: history.grid?.bucketMs ?? live.intervalMs,
    loading: history.loading,
    error: history.error,
  };
}
