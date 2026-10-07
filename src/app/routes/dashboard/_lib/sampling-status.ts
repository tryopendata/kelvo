import type { StatusPillProps } from "~/widgets/status-pill";

/**
 * The dashboard page header's status pill: Paused while
 * sampling is off, Reconnecting while the live channel resubscribes, Stale
 * after three intervals without a frame, otherwise Live.
 *
 * Stale is the channel's liveness, not a value's: the host store sets it
 * after `STALE_INTERVALS` of Rust's `LiveStatus.frame_period_ms` pass with
 * no frame. Whether a series' value is still current is Rust's rule
 * (`held`, nulled past `HOLD_FACTOR` times its period; see `read-failure.ts`).
 */
export function samplingStatus(s: {
  paused: boolean;
  stale: boolean;
  connection?: "connecting" | "live" | "reconnecting";
}): StatusPillProps {
  if (s.paused) return { state: "paused", label: "Paused" };
  if (s.connection === "reconnecting")
    return { state: "stale", label: "Reconnecting" };
  if (s.stale) return { state: "stale", label: "Stale" };
  return { state: "live", label: "Live" };
}

/**
 * The sidebar footer names the battery only when it slowed sampling down
 * (plan 4.4: "Sampling every 2s · on battery"); on battery at the configured
 * interval it just says the interval.
 */
export function batteryBackoff(s: {
  onBattery: boolean;
  intervalMs: number | null;
  configuredMs: number | null;
}): boolean {
  if (!s.onBattery || s.intervalMs === null) return false;
  return s.configuredMs === null || s.intervalMs > s.configuredMs;
}
