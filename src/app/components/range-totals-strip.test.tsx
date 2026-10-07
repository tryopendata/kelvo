import { measuredShort } from "./range-totals-strip";

const stat = (measured_ms: number) => ({
  metric: "cpu.total",
  measured_ms,
  avg: 10,
  max: 20,
  integral: 0,
});

describe("measuredShort", () => {
  it("is null when the metric covered the range, within slack", () => {
    expect(measuredShort(stat(900_000), 900_000)).toBeNull();
    expect(measuredShort(stat(892_000), 900_000)).toBeNull();
    expect(measuredShort(stat(0), 0)).toBeNull();
  });

  it("names the measured time when it fell short", () => {
    expect(measuredShort(stat(660_000), 900_000)).toBe("11 min");
    expect(measuredShort(stat(0), 900_000)).toBe("0 s");
  });
});
