/**
 * The engine's sampling plans (D-092): its tick with a window open and in the
 * background (D-094), the menu bar's redraw period, a visible window's frame
 * period and the background collector periods, for
 * every combination of settings and power state. Generated from the Rust
 * functions the engine, the tray and the live stream run on, so Settings can
 * say what another choice would do by reading a row.
 */
import { SAMPLING_PLANS, type SamplingPlan } from "@core/generated/bindings";

export type { SamplingPlan };

const PLANS: readonly SamplingPlan[] = SAMPLING_PLANS;

export interface PlanQuery {
  interval_ms: number;
  slow_on_battery: boolean;
  /** Performance mode in effect: the setting, or Low Power Mode. */
  performance: boolean;
  low_power_mode: boolean;
}

/** The plan for `q`; null for an interval Rust does not offer. */
export function samplingPlan(q: PlanQuery): SamplingPlan | null {
  return (
    PLANS.find(
      (p) =>
        p.interval_ms === q.interval_ms &&
        p.slow_on_battery === q.slow_on_battery &&
        p.performance === q.performance &&
        p.low_power_mode === q.low_power_mode
    ) ?? null
  );
}
