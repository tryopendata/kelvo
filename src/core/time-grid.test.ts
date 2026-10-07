import { ceilTo, floorTo } from "@core/time-grid";

describe("floorTo and ceilTo", () => {
  it("snaps to the grid line at or before and at or after", () => {
    expect(floorTo(25_000, 10_000)).toBe(20_000);
    expect(ceilTo(25_000, 10_000)).toBe(30_000);
    expect(floorTo(20_000, 10_000)).toBe(20_000);
    expect(ceilTo(20_000, 10_000)).toBe(20_000);
  });

  it("snaps negative times down, not toward zero", () => {
    expect(floorTo(-5_000, 10_000)).toBe(-10_000);
    expect(ceilTo(-5_000, 10_000)).toBe(0);
  });
});
