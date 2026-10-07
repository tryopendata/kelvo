import { formatHoursMinutes, MISSING, wattsParts } from "@core/format";
import type { PowerSource } from "@core/generated/bindings";
import type { BatteryDetail } from "~/hooks/use-battery";

/** `LiveStatus.power_source` (D-092), "unknown" before the first status. */
export type PowerState = PowerSource | "unknown";

export const STATE_TEXT: Record<PowerState, string> = {
  charging: "Charging",
  adapter: "On power adapter, not charging",
  battery: "On battery",
  unknown: "Power source unknown",
};

/**
 * `battery.time_remaining` is time to empty on battery and time to full
 * while charging (IOPowerSources), so its label follows the state. On the
 * adapter without charging there is no countdown.
 */
export function timeField(
  b: BatteryDetail,
  state: PowerState
): { label: string; value: string } {
  const value =
    b.timeRemainingMin === null || state === "adapter"
      ? MISSING
      : formatHoursMinutes(b.timeRemainingMin * 60_000);
  return { label: state === "charging" ? "To full" : "Remaining", value };
}

/** "On battery · 6:12 remaining", "Charging · 1:05 to full". */
export function batterySubtitle(b: BatteryDetail, state: PowerState): string {
  const time = timeField(b, state);
  const parts = [STATE_TEXT[state]];
  if (time.value !== MISSING && state !== "unknown") {
    parts.push(
      `${time.value} ${time.label === "To full" ? "to full" : "remaining"}`
    );
  }
  return parts.join(" · ");
}

/**
 * The power ring's text. `battery.power` is signed: negative drains the
 * battery, positive charges it.
 */
export function powerText(powerW: number | null): {
  value: string;
  description: string;
} {
  if (powerW === null) return { value: MISSING, description: "No reading" };
  const value = wattsParts(Math.abs(powerW)).value;
  if (Math.abs(powerW) < 0.05) {
    return { value, description: "No current into or out of the battery" };
  }
  return {
    value,
    description: powerW < 0 ? "Drawn from the battery" : "Charging the battery",
  };
}
