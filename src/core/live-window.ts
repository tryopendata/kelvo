/**
 * Live chart windows that follow the sampling interval (plan 4.15). A
 * window designed at 1 s keeps its sample count at slower intervals: the
 * popover's 60 s charts hold 60 samples, which at 30 s cover 30 minutes.
 * Faster intervals keep the window (more samples), and no window exceeds the
 * engine's one-hour ring.
 */
import { type ChartWindow, RING_SPAN_MS } from "@core/generated/bindings";
import { samplingPlan } from "@core/sampling-plans";

/**
 * Fewest samples a fixed window choice (5m, 15m, 30m, 1h) must hold to be
 * offered. Under this the chart is a few dots: 5m at 60 s is 5 samples.
 */
export const MIN_WINDOW_SAMPLES = 10;

/** `baseMs` (designed for 1 s) stretched to the same sample count at `intervalMs`. */
export function scaledWindowMs(baseMs: number, intervalMs: number): number {
  return Math.min(RING_SPAN_MS, baseMs * Math.max(1, intervalMs / 1000));
}

/** Whether a fixed window holds enough samples at `intervalMs`. The 1 h window always does. */
export function windowAllowed(windowMs: number, intervalMs: number): boolean {
  return (
    windowMs >= RING_SPAN_MS || windowMs / intervalMs >= MIN_WINDOW_SAMPLES
  );
}

/**
 * The module pages' chart window (D-091), shortest first. One choice, saved
 * in settings, applies to every windowed chart on every module page.
 */
export const CHART_WINDOWS: readonly ChartWindow[] = ["5m", "15m", "30m", "1h"];

/** The chart window when none is saved or settings could not be read (Rust's default). */
export const DEFAULT_CHART_WINDOW: ChartWindow = "15m";

export const CHART_WINDOW_MS: Record<ChartWindow, number> = {
  "5m": 300_000,
  "15m": 900_000,
  "30m": 1_800_000,
  "1h": 3_600_000,
};

/**
 * The window the pages show for the `saved` choice at `intervalMs`: the
 * saved one when it holds enough samples, otherwise the shortest that does.
 * The saved choice is never overwritten, so it comes back when the interval
 * speeds up again.
 */
export function effectiveWindow(
  saved: ChartWindow,
  intervalMs: number
): ChartWindow {
  if (windowAllowed(CHART_WINDOW_MS[saved], intervalMs)) return saved;
  return (
    CHART_WINDOWS.find((w) => windowAllowed(CHART_WINDOW_MS[w], intervalMs)) ??
    "1h"
  );
}

/**
 * The slowest tick the engine may run at for these settings: its tick on
 * battery from the sampling plans (D-092). Low Power Mode is not a setting;
 * pass it when the caller knows it.
 */
export function slowestIntervalMs(
  sampling: {
    interval_ms: number;
    slow_on_battery: boolean;
    performance_mode?: boolean;
  },
  lowPowerMode = false
): number {
  const plan = samplingPlan({
    interval_ms: sampling.interval_ms,
    slow_on_battery: sampling.slow_on_battery,
    performance: !!sampling.performance_mode || lowPowerMode,
    low_power_mode: lowPowerMode,
  });
  return plan?.battery.tick_ms ?? sampling.interval_ms;
}

/** Corner label for a window: "48s", "60s", "30m", "1h". */
export function windowLabel(ms: number): string {
  if (ms < 120_000) return `${Math.round(ms / 1000)}s`;
  if (ms < RING_SPAN_MS) return `${Math.round(ms / 60_000)}m`;
  return `${Math.round(ms / RING_SPAN_MS)}h`;
}

/** The same window in words, for accessible names: "60 seconds", "30 minutes", "hour". */
export function windowWords(ms: number): string {
  if (ms < 120_000) return `${Math.round(ms / 1000)} seconds`;
  if (ms < RING_SPAN_MS) return `${Math.round(ms / 60_000)} minutes`;
  const h = Math.round(ms / RING_SPAN_MS);
  return h === 1 ? "hour" : `${h} hours`;
}
