import { networkTotal } from "./lane-label";

describe("network lane's current value", () => {
  it("adds download and upload when both report", () => {
    expect(networkTotal({ rx: 2e6, tx: 0.5e6 })).toBe(2.5e6);
  });

  it("is null when either direction is missing", () => {
    expect(networkTotal({ rx: 2e6, tx: null })).toBeNull();
    expect(networkTotal({ rx: null, tx: 0.5e6 })).toBeNull();
  });
});
