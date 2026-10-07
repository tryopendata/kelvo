import { pressureState } from "./pressure";

describe("pressureState", () => {
  it("maps the kernel level to a state word", () => {
    expect([0, 1, 2].map(pressureState)).toEqual([
      "normal",
      "warn",
      "critical",
    ]);
  });

  it("has no state without a reading", () => {
    expect(pressureState(null)).toBeNull();
    expect(pressureState(Number.NaN)).toBeNull();
  });
});
