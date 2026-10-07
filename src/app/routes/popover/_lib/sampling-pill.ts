import type { PerformanceReason } from "@core/generated/bindings";
import type { StatusPillProps } from "~/widgets/status-pill";

/**
 * The interval pill: "Paused", "Reconnecting" (the live channel dropped and
 * is resubscribing), "Stale" (no frame for three intervals), or the
 * effective interval ("1s", "2s"), with "· perf" in Performance mode
 * ("2s · perf", D-088).
 */
export function samplingPill(s: {
  intervalMs: number | null;
  /** The window's frame period; in Performance mode it is what the pill shows. */
  framePeriodMs?: number | null;
  paused: boolean;
  stale: boolean;
  connection?: "connecting" | "live" | "reconnecting";
  performance?: PerformanceReason;
}): StatusPillProps {
  if (s.paused) return { state: "paused", label: "Paused" };
  if (s.connection === "reconnecting")
    return { state: "stale", label: "Reconnecting" };
  if (s.stale) return { state: "stale", label: "Stale" };
  const perf = (s.performance ?? "off") !== "off";
  const ms = (perf ? s.framePeriodMs : null) ?? s.intervalMs ?? 1000;
  return { state: "live", label: `${ms / 1000}s${perf ? " · perf" : ""}` };
}
