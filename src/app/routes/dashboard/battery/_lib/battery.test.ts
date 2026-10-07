import { describe, expect, it } from "vitest";
import type { BatteryDetail } from "~/hooks/use-battery";
import { batterySubtitle, powerText, timeField } from "./battery";

const base: BatteryDetail = {
  charge: 87,
  charging: 0,
  external: 0,
  timeRemainingMin: 372,
  health: 94,
  cycles: 212,
  capacityWh: 68.2,
  designWh: 72.6,
  powerW: -14.8,
  tempC: null,
  systemW: 14.8,
};

describe("battery page helpers", () => {
  it("labels time_remaining as time to full while charging", () => {
    expect(timeField(base, "battery")).toEqual({
      label: "Remaining",
      value: "6:12",
    });
    const charging = { ...base, timeRemainingMin: 65 };
    expect(timeField(charging, "charging")).toEqual({
      label: "To full",
      value: "1:05",
    });
    expect(batterySubtitle(base, "battery")).toBe(
      "On battery · 6:12 remaining"
    );
    expect(batterySubtitle(charging, "charging")).toBe(
      "Charging · 1:05 to full"
    );
    expect(batterySubtitle(base, "adapter")).toBe(
      "On power adapter, not charging"
    );
    expect(batterySubtitle(base, "unknown")).toBe("Power source unknown");
  });

  it("describes the signed battery power", () => {
    expect(powerText(-14.8)).toEqual({
      value: "14.8",
      description: "Drawn from the battery",
    });
    expect(powerText(30).description).toBe("Charging the battery");
    expect(powerText(null).description).toBe("No reading");
  });
});
