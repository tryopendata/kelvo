import type { ChartWindow } from "@core/generated/bindings";
import { gridIntervalMs } from "@core/live-state";
import {
  CHART_WINDOW_MS,
  DEFAULT_CHART_WINDOW,
  effectiveWindow,
} from "@core/live-window";
import { useCallback, useMemo } from "react";
import { useWriteSettings } from "~/hooks/use-write-settings";
import { useHost } from "~/stores/host-store";
import { useSettings, useSettingsStatus } from "~/stores/settings-store";

export interface ChartWindowState {
  window: ChartWindow;
  windowMs: number;
}

/**
 * The module pages' chart window (D-091): the saved choice, or the shortest
 * allowed one while sampling is too slow for it. `null` while settings load,
 * so a page waits instead of drawing a span it would redo. If they could not
 * be read, the default window, so the pages still draw.
 */
export function useChartWindow(): ChartWindowState | null {
  // Rust always writes the field; `undefined` is only the serde default's type.
  const saved = useSettings(
    (s) => s.general.chart_window ?? DEFAULT_CHART_WINDOW
  );
  const failed = useSettingsStatus() === "failed";
  const intervalMs = useHost((s) => gridIntervalMs(s.status));
  const chosen = saved ?? (failed ? DEFAULT_CHART_WINDOW : null);
  const window = chosen === null ? null : effectiveWindow(chosen, intervalMs);
  return useMemo(
    () =>
      window === null ? null : { window, windowMs: CHART_WINDOW_MS[window] },
    [window]
  );
}

/**
 * Save a chart window. The mirror takes the returned snapshot, so every page
 * and dashboard window follows; a failed write keeps the old window and says
 * why in the settings toast.
 */
export function useSetChartWindow(): (window: ChartWindow) => void {
  const write = useWriteSettings();
  return useCallback(
    (window) => write({ general: { chart_window: window } }),
    [write]
  );
}
