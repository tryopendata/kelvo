import { METRIC_CODES } from "@core/generated/bindings";

/**
 * `mem.pressure_level` (from `kern.memorystatus_vm_pressure_level`, D-045;
 * codes in `METRIC_CODES`) as the state word the popover and Memory page
 * show next to the pressure figure. Warn and critical also need an icon
 * (design-system.md "Status colors"); normal needs nothing.
 */
export type PressureState = keyof (typeof METRIC_CODES)["mem.pressure_level"];

const LEVEL = METRIC_CODES["mem.pressure_level"];

export function pressureState(level: number | null): PressureState | null {
  if (level === null || !Number.isFinite(level)) return null;
  if (level >= LEVEL.critical) return "critical";
  if (level >= LEVEL.warn) return "warn";
  return "normal";
}
