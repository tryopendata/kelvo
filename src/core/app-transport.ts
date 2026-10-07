import { CHART_WINDOWS } from "./live-window";
import type { ScenarioName } from "./mock/fixtures";
import { SCENARIOS } from "./mock/fixtures";
import { createTauriTransport, isTauri, type Transport } from "./transport";

/**
 * The transport for this window. Inside Tauri: the app transport, labelled by
 * the Tauri window. In a browser (dev server, Playwright, `VITE_TRANSPORT=
 * mock`): the mock, configured from the query string:
 *
 *   ?window=popover|dashboard|onboarding   window label (default dashboard)
 *   ?scenario=no-fans,paused               mock scenarios, comma separated
 *   ?ticks=0                               no auto-tick (stable screenshots)
 *   ?interval=30000                        starting sample interval, ms
 *   ?chart_window=30m                      starting chart window (5m|15m|30m|1h)
 */
export async function createAppTransport(search: string): Promise<Transport> {
  if (isTauri() && import.meta.env.VITE_TRANSPORT !== "mock") {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    return createTauriTransport(getCurrentWindow().label);
  }
  const params = new URLSearchParams(search);
  const scenarios = (params.get("scenario") ?? "default")
    .split(",")
    .filter((s): s is ScenarioName =>
      (SCENARIOS as readonly string[]).includes(s)
    );
  const { createMockTransport } = await import("./mock-transport");
  const windowLabel = params.get("window") ?? "dashboard";
  return createMockTransport({
    windowLabel,
    // The dashboard backfills an hour of ring (app.tsx); give the mock one.
    historyRows: windowLabel === "dashboard" ? 3600 : undefined,
    scenarios,
    autoTick: params.get("ticks") !== "0",
    intervalMs: Number(params.get("interval")) || undefined,
    chartWindow: CHART_WINDOWS.find((w) => w === params.get("chart_window")),
  });
}
