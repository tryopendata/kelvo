import { type CeilingState, nextCeiling } from "@core/chart-math";
import { useRef } from "react";

/**
 * Autoscaled chart ceiling with the 60 s shrink hysteresis
 * (design-system.md "Chart rules"). Feed it the values in the chart's window
 * and the time of the newest one; it grows at once and shrinks only after a
 * minute below the next step down.
 *
 * Pass the chart's window as `windowMs`: switching windows drops the held
 * ceiling and fits the new window's data at once. The hysteresis only damps
 * jitter while one window streams.
 */
export function useNiceCeiling(
  values: readonly (number | null)[],
  nowMs: number,
  minCeiling = 1,
  windowMs?: number
): number {
  const state = useRef<CeilingState | null>(null);
  const lastMs = useRef<number | null>(null);
  const lastMax = useRef<number | null>(null);
  const lastWindow = useRef(windowMs);
  if (lastWindow.current !== windowMs) {
    state.current = null;
    lastMs.current = null;
    lastWindow.current = windowMs;
  }
  let max: number | null = null;
  for (const v of values) if (v !== null && (max === null || v > max)) max = v;
  // Selection changes can publish a larger series at the same history timestamp.
  if (lastMs.current !== nowMs || lastMax.current !== max) {
    state.current = nextCeiling(state.current, max, nowMs, { minCeiling });
    lastMs.current = nowMs;
    lastMax.current = max;
  }
  return state.current?.ceiling ?? minCeiling;
}
