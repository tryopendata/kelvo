import { stackTotals } from "./power-stack-card";

describe("stackTotals", () => {
  it("sums each slot and leaves a gap where any component is missing", () => {
    expect(
      stackTotals([
        [1, 2, null],
        [0.5, null, 3],
      ])
    ).toEqual([1.5, null, null]);
  });

  it("is empty with no series", () => {
    expect(stackTotals([])).toEqual([]);
  });
});
