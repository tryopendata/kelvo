import {
  BRUSH_STEP_MS,
  brushBucketMs,
  bucketAt,
  clampRange,
  extendRange,
  rangeFractions,
  rangeSlots,
  snapRange,
  timeAt,
} from "./brush";

const T = 1_700_000_000_000; // a 10 s edge
const S = 1000;

// What `mirrorBuckets` picks without a brush at 1 s: 3 s, 8 s, 15 s, 30 s.
describe("brush bar widths (D-089)", () => {
  it.each([
    ["5m", 3000, 5000],
    ["15m", 8000, 10_000],
    ["30m", 15_000, 30_000],
    ["1h", 30_000, 30_000],
  ])("%s: %i ms bars become %i ms when brushable", (_, plain, want) => {
    expect(brushBucketMs(plain, 1000)).toBe(want);
  });

  it("never goes below the sample interval", () => {
    expect(brushBucketMs(1000, 2000)).toBe(2000);
    expect(brushBucketMs(1000, 10_000)).toBe(10_000);
  });

  it("every width under 10 s divides it, every larger one is a multiple", () => {
    for (const w of [1, 1000, 2500, 8000, 20_000, 45_000, 120_000].map((m) =>
      brushBucketMs(m, 1000)
    )) {
      expect(w < BRUSH_STEP_MS ? BRUSH_STEP_MS % w : w % BRUSH_STEP_MS).toBe(0);
    }
  });
});

describe("snapping", () => {
  it("snaps a drag outward to 10 s edges, in either direction", () => {
    const want = { fromMs: T + 10 * S, toMs: T + 40 * S };
    expect(snapRange(T + 13 * S, T + 31 * S)).toEqual(want);
    expect(snapRange(T + 31 * S, T + 13 * S)).toEqual(want);
  });

  it("keeps edges that are already on a bucket edge", () => {
    expect(snapRange(T, T + 20 * S)).toEqual({ fromMs: T, toMs: T + 20 * S });
  });

  it("never yields an empty range", () => {
    expect(snapRange(T + 4 * S, T + 4 * S)).toEqual({
      fromMs: T,
      toMs: T + 10 * S,
    });
    expect(snapRange(T, T)).toEqual({ fromMs: T, toMs: T + 10 * S });
  });

  it("a click selects the bucket that holds it", () => {
    expect(bucketAt(T + 19_999)).toEqual({
      fromMs: T + 10 * S,
      toMs: T + 20 * S,
    });
    expect(bucketAt(T - 1)).toEqual({ fromMs: T - 10 * S, toMs: T });
  });

  it("extends from an anchor bucket to the focused one, both included", () => {
    expect(extendRange(T + 20 * S, T + 40 * S)).toEqual({
      fromMs: T + 20 * S,
      toMs: T + 50 * S,
    });
    expect(extendRange(T + 20 * S, T)).toEqual({ fromMs: T, toMs: T + 30 * S });
  });

  it("clamps to the chart, snapped, keeping one bucket", () => {
    const lo = T + 3 * S;
    const hi = T + 57 * S;
    expect(
      clampRange({ fromMs: T - 50 * S, toMs: T + 90 * S }, lo, hi)
    ).toEqual({
      fromMs: T,
      toMs: T + 60 * S,
    });
    expect(
      clampRange({ fromMs: T + 80 * S, toMs: T + 90 * S }, lo, hi)
    ).toEqual({
      fromMs: T + 50 * S,
      toMs: T + 60 * S,
    });
  });
});

describe("chart geometry", () => {
  it("maps a pointer fraction to time, clamped to the chart", () => {
    expect(timeAt(0.5, T, 60 * S)).toBe(T + 30 * S);
    expect(timeAt(-1, T, 60 * S)).toBe(T);
    expect(timeAt(2, T, 60 * S)).toBe(T + 60 * S);
  });

  it("places a range on the chart, cut to it, or nowhere once scrolled off", () => {
    expect(
      rangeFractions({ fromMs: T + 15 * S, toMs: T + 30 * S }, T, 60 * S)
    ).toEqual({
      left: 0.25,
      width: 0.25,
    });
    expect(
      rangeFractions({ fromMs: T - 30 * S, toMs: T + 15 * S }, T, 60 * S)
    ).toEqual({
      left: 0,
      width: 0.25,
    });
    expect(
      rangeFractions({ fromMs: T - 30 * S, toMs: T }, T, 60 * S)
    ).toBeNull();
  });

  it("finds the bar slots a range touches", () => {
    // 5 s bars from T: a 10 s-aligned range covers whole bars.
    expect(
      rangeSlots({ fromMs: T + 10 * S, toMs: T + 30 * S }, T, 5000, 60)
    ).toEqual({
      from: 2,
      to: 6,
    });
    // 30 s bars: a 10 s range lights the bar it falls in.
    expect(
      rangeSlots({ fromMs: T + 40 * S, toMs: T + 50 * S }, T, 30_000, 120)
    ).toEqual({
      from: 1,
      to: 2,
    });
    expect(
      rangeSlots({ fromMs: T - 90 * S, toMs: T - 60 * S }, T, 5000, 60)
    ).toEqual({
      from: 0,
      to: 0,
    });
  });
});
