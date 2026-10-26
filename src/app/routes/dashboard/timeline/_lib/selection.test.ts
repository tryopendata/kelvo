import { selectedRange } from "./selection";

describe("Timeline selection", () => {
  it("snaps reverse drags outward and enforces a minute", () => {
    expect(selectedRange(92_000, 63_000, 10_000, 600_000)).toEqual({
      fromMs: 60_000,
      toMs: 120_000,
    });
  });
  it("never invents detail inside a coarser retained bucket", () => {
    expect(selectedRange(970_000, 980_000, 900_000, 3_600_000)).toEqual({
      fromMs: 900_000,
      toMs: 1_800_000,
    });
  });
  it("clamps the future and refuses an invalid retained interval", () => {
    expect(selectedRange(590_000, 700_000, 10_000, 600_000)).toEqual({
      fromMs: 540_000,
      toMs: 600_000,
    });
    expect(
      selectedRange(590_000, 700_000, 10_000, 600_000, 570_000)
    ).toBeNull();
  });
});
