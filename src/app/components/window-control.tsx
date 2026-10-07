import { gridIntervalMs } from "@core/live-state";
import {
  CHART_WINDOW_MS,
  CHART_WINDOWS,
  windowAllowed,
} from "@core/live-window";
import { useMemo } from "react";
import { SegmentedControl } from "~/components/segmented-control";
import { useChartWindow, useSetChartWindow } from "~/hooks/use-chart-window";
import { useHost } from "~/stores/host-store";

/**
 * The 5m / 15m / 30m / 1h control in a module page header (D-091).
 * It reads and saves the one chart window every module page shares. A window
 * that would hold under 10 samples at the current interval is disabled; the
 * pages then show the shortest allowed window without overwriting the saved
 * choice. Renders nothing while settings load; if they failed to load it
 * shows the default window, and a pick still goes through `update_settings`.
 */
export function WindowControl() {
  const intervalMs = useHost((s) => gridIntervalMs(s.status));
  const current = useChartWindow()?.window ?? null;
  const setWindow = useSetChartWindow();
  const options = useMemo(
    () =>
      CHART_WINDOWS.map((w) => ({
        value: w,
        label: w,
        disabled: !windowAllowed(CHART_WINDOW_MS[w], intervalMs),
      })),
    [intervalMs]
  );
  if (current === null) return null;
  return (
    <SegmentedControl
      options={options}
      value={current}
      onChange={setWindow}
      ariaLabel="Window"
    />
  );
}
