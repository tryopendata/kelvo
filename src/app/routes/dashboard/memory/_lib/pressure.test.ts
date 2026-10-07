import { marketingGb, pressureState } from "./pressure";

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

describe("marketingGb", () => {
  it("rounds the byte total to whole GiB", () => {
    expect(marketingGb(24 * 2 ** 30 - 5e6)).toBe(24);
  });
});
