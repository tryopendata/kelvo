import { batteryBackoff, samplingStatus } from "./sampling-status";

describe("samplingStatus", () => {
  it("is Live while frames arrive", () => {
    expect(samplingStatus({ paused: false, stale: false })).toEqual({
      state: "live",
      label: "Live",
    });
  });

  it("puts paused ahead of stale, and reconnecting ahead of stale", () => {
    expect(samplingStatus({ paused: true, stale: true }).label).toBe("Paused");
    expect(
      samplingStatus({ paused: false, stale: true, connection: "reconnecting" })
        .label
    ).toBe("Reconnecting");
    expect(samplingStatus({ paused: false, stale: true })).toEqual({
      state: "stale",
      label: "Stale",
    });
  });
});

describe("batteryBackoff", () => {
  it("names the battery only when it slowed sampling down", () => {
    expect(
      batteryBackoff({ onBattery: true, intervalMs: 2000, configuredMs: 1000 })
    ).toBe(true);
    expect(
      batteryBackoff({ onBattery: true, intervalMs: 1000, configuredMs: 1000 })
    ).toBe(false);
    expect(
      batteryBackoff({ onBattery: false, intervalMs: 2000, configuredMs: 1000 })
    ).toBe(false);
  });
});
