import { describe, expect, it } from "vitest";
import { fanSummary, sensorLabel, zoneName } from "./sensors";

describe("labels", () => {
  it("names zones by position with two digits", () => {
    expect(zoneName(0)).toBe("Zone 01");
    expect(zoneName(11)).toBe("Zone 12");
  });

  it("maps catalog sensor names to display labels", () => {
    expect(sensorLabel("ssd")).toBe("SSD (NAND)");
    expect(sensorLabel("wifi")).toBe("Wi‑Fi module");
    expect(sensorLabel("lid")).toBe("lid");
  });

  it("summarises one, two or more fans", () => {
    expect(fanSummary([1840, 1860])).toBe("Left 1,840 · right 1,860");
    expect(fanSummary([1200])).toBe("Fan 1,200");
    expect(fanSummary([1, null, 3])).toBe("1: 1 · 2: — · 3: 3");
  });
});
