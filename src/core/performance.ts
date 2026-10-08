import type {
  PerformanceReason,
  PlanFigures,
  Settings,
} from "@core/generated/bindings";
import { samplingPlan } from "@core/sampling-plans";
import { menuBarOf, SETTINGS_MODULES } from "@core/settings-patch";

const secs = (ms: number) => `${ms / 1000} s`;

/**
 * What Performance mode changes for these settings (D-088), as short lines:
 * each figure where the sampling plan with the mode on differs from the one
 * without it (D-092). Under Low Power Mode the engine is backed off on AC
 * too, so its figures are the backed-off ones. The background's slow menu
 * bar, process and temperature cadences apply with the mode off too (D-094),
 * so they are not among them. The Settings disclosure and the indicators'
 * explainer both read this, so they agree.
 */
export function performanceChanges(
  settings: Pick<Settings, "sampling" | "modules">,
  reason: PerformanceReason = "off"
): string[] {
  const { sampling } = settings;
  const query = {
    interval_ms: sampling.interval_ms,
    slow_on_battery: sampling.slow_on_battery,
    low_power_mode: reason === "low_power_mode",
  };
  const on = samplingPlan({ ...query, performance: true });
  const off = samplingPlan({ ...query, performance: false });
  const out: string[] = [];
  if (on && off) {
    const changed = (f: keyof PlanFigures) => on.ac[f] !== off.ac[f];
    if (changed("window_ms")) {
      out.push(`Open windows update every ${secs(on.ac.window_ms)}`);
    }
    if (changed("window_menu_bar_ms")) {
      out.push(
        `Menu bar updates every ${secs(on.ac.window_menu_bar_ms)} with a window open`
      );
    }
    if (changed("window_idle_processes_ms")) {
      out.push(
        `Processes sampled every ${secs(on.ac.window_idle_processes_ms)} when no view lists them`
      );
    }
    if (on.battery.tick_ms !== off.battery.tick_ms) {
      out.push("Interval doubles on battery");
    }
  }
  out.push("Animations off");
  return out;
}

/**
 * The next thing to change for a lower cost, which Performance mode leaves to
 * the user: separate menu bar items first, then the interval. Null when
 * neither applies.
 */
export function performanceNextLever(
  settings: Pick<Settings, "sampling" | "modules" | "menu_bar">
): string | null {
  // Own status items (D-080) of modules that are on.
  const items = menuBarOf(settings).items;
  const own = SETTINGS_MODULES.some(
    (m) => settings.modules[m]?.enabled && items[m] !== "off"
  );
  if (own) {
    return "Separate menu bar items cost the most; turn them off under Menu bar to save more.";
  }
  // In the background the tick is already at least 2 s (D-094).
  if (settings.sampling.interval_ms < 2000) {
    return "A longer sample interval saves more while a window is open.";
  }
  return null;
}

/** Why Performance mode is on, for the indicators' explainer. */
export function performanceWhy(reason: PerformanceReason): string | null {
  switch (reason) {
    case "setting":
      return "You turned on Performance mode in Settings.";
    case "low_power_mode":
      return "Performance mode turned on because macOS Low Power Mode is on. It turns off when Low Power Mode does.";
    default:
      return null;
  }
}
