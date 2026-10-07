import { sum } from "./live-sidebar";

describe("sidebar sum of both directions", () => {
  it("adds the two when both report", () => {
    expect(sum(38.4e6, 1.2e6)).toBe(39.6e6);
  });

  it("is null when either direction is missing, never the other alone", () => {
    expect(sum(38.4e6, null)).toBeNull();
    expect(sum(null, 1.2e6)).toBeNull();
    expect(sum(null, null)).toBeNull();
  });
});
