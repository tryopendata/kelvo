import { describe, expect, it } from "vitest";
import { fanSummary, nextZoneOrder, sensorLabel, zoneName } from "./sensors";

const zones = (temps: Record<string, number | null>) =>
  Object.entries(temps).map(([key, now]) => ({ key, now }));

describe("nextZoneOrder", () => {
  it("sorts hottest first with missing readings last", () => {
    const order = nextZoneOrder(
      null,
      zones({ a: 50, b: null, c: 61, d: 55 }),
      0
    );
    expect(order.keys).toEqual(["c", "d", "a", "b"]);
  });

  it("holds the order for 10 s even when the ranking changes", () => {
    const first = nextZoneOrder(null, zones({ a: 60, b: 50 }), 1_000);
    const swapped = zones({ a: 50, b: 60 });
    expect(nextZoneOrder(first, swapped, 10_999).keys).toEqual(["a", "b"]);
    expect(nextZoneOrder(first, swapped, 11_000).keys).toEqual(["b", "a"]);
  });

  it("re-sorts at once when a zone appears or goes away", () => {
    const first = nextZoneOrder(null, zones({ a: 60, b: 50 }), 1_000);
    expect(
      nextZoneOrder(first, zones({ a: 40, b: 50, c: 70 }), 2_000).keys
    ).toEqual(["c", "b", "a"]);
    expect(nextZoneOrder(first, zones({ b: 50 }), 2_000).keys).toEqual(["b"]);
  });
});

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
