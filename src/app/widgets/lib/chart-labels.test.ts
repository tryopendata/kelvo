import { ceilingAxis, PERCENT_Y_TICKS } from "./chart-labels";

describe("ceilingAxis", () => {
  it("puts gridlines at half and full height", () => {
    expect(ceilingAxis(40)).toEqual({ gridlines: [20, 40] });
  });

  it("labels the ceiling with its unit and the half bare", () => {
    expect(ceilingAxis(5, "5W")).toEqual({
      gridlines: [2.5, 5],
      yTicks: [
        { value: 5, label: "5W" },
        { value: 2.5, label: "2.5" },
      ],
    });
  });
});

describe("PERCENT_Y_TICKS", () => {
  it("reads top down without the unit", () => {
    expect(PERCENT_Y_TICKS.map((t) => t.label)).toEqual([
      "100",
      "75",
      "50",
      "25",
    ]);
  });
});
