import { cpuPowerNote } from "./cpu-power-source";

describe("cpuPowerNote", () => {
  it("says P cores for every SMC source, plus the calibration state", () => {
    expect(cpuPowerNote(1)).toBe("CPU power: P cores, uncalibrated");
    expect(cpuPowerNote(2)).toBe("CPU power: P cores");
    expect(cpuPowerNote(3)).toBe(
      "CPU power: P cores, estimated from last calibration"
    );
  });

  it("says nothing when PMP measures CPU power (series absent)", () => {
    expect(cpuPowerNote(null)).toBeNull();
  });

  it("treats an unknown source as uncalibrated", () => {
    expect(cpuPowerNote(4)).toBe("CPU power: P cores, uncalibrated");
    expect(cpuPowerNote(0)).toBe("CPU power: P cores, uncalibrated");
  });
});
