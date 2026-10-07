import {
  type CeilingState,
  clamp01,
  downsampleMinMaxAvg,
  heatmapAlpha,
  nextCeiling,
  niceCeiling,
  percentOf,
  ratio,
  ringArcs,
  splitGaps,
  stackRemainder,
  stackSum,
  stepBelow,
} from "@core/chart-math";

describe("clamp01", () => {
  it("clamps to [0, 1] and turns non-finite input into 0", () => {
    expect(clamp01(0.4)).toBe(0.4);
    expect(clamp01(-2)).toBe(0);
    expect(clamp01(7)).toBe(1);
    expect(clamp01(Number.NaN)).toBe(0);
    expect(clamp01(Number.POSITIVE_INFINITY)).toBe(0);
  });
});

describe("ratio and percentOf", () => {
  it("is null when either side is missing, so the bar draws no fill", () => {
    expect(ratio(null, 10)).toBeNull();
    expect(ratio(5, null)).toBeNull();
    expect(ratio(5, 10)).toBe(0.5);
    expect(percentOf(5, 10)).toBe(50);
    expect(percentOf(null, 10)).toBeNull();
  });

  it("is null for a whole of zero or less, never Infinity or NaN", () => {
    expect(ratio(5, 0)).toBeNull();
    expect(ratio(0, 0)).toBeNull();
    expect(ratio(5, -1)).toBeNull();
    expect(percentOf(5, 0)).toBeNull();
  });
});

describe("stack remainder", () => {
  it("is the whole less its parts, floored at 0", () => {
    expect(stackRemainder(14, [5, 3, 0, 1])).toBe(5);
    expect(stackRemainder(4, [5, 3])).toBe(0);
  });

  it("is unknown when the whole or any part is missing", () => {
    expect(stackRemainder(14, [5, null, 0, 1])).toBeNull();
    expect(stackRemainder(null, [5, 3])).toBeNull();
    expect(stackSum([1, undefined])).toBeNull();
    expect(stackSum([1, 2])).toBe(3);
  });
});

describe("splitGaps", () => {
  it("splits on null and keeps the original indices", () => {
    expect(splitGaps([1, 2, null, null, 5, 6, null, 8])).toEqual([
      { start: 0, values: [1, 2] },
      { start: 4, values: [5, 6] },
      { start: 7, values: [8] },
    ]);
  });

  it("keeps a real zero inside a run", () => {
    expect(splitGaps([0, 0, 3])).toEqual([{ start: 0, values: [0, 0, 3] }]);
  });

  it("treats NaN as a gap", () => {
    expect(splitGaps([1, Number.NaN, 2])).toHaveLength(2);
  });

  it("returns nothing for an all-gap or empty series", () => {
    expect(splitGaps([null, null])).toEqual([]);
    expect(splitGaps([])).toEqual([]);
  });
});

describe("downsampleMinMaxAvg", () => {
  it("keeps a one-sample spike in the max", () => {
    const series = Array.from({ length: 100 }, (_, i) => (i === 37 ? 95 : 10));
    const out = downsampleMinMaxAvg(series, 10);
    expect(out).toHaveLength(10);
    expect(out[3]).toEqual({ min: 10, max: 95, avg: 18.5 });
    expect(out[4]).toEqual({ min: 10, max: 10, avg: 10 });
  });

  it("assigns every sample to exactly one bucket when n is not a multiple", () => {
    const series = [1, 2, 3, 4, 5, 6, 7];
    const out = downsampleMinMaxAvg(series, 3);
    expect(out).toEqual([
      { min: 1, max: 2, avg: 1.5 },
      { min: 3, max: 4, avg: 3.5 },
      { min: 5, max: 7, avg: 6 },
    ]);
  });

  it("skips nulls and returns null for an all-gap bucket", () => {
    const out = downsampleMinMaxAvg([4, null, null, null, 2, 6], 3);
    expect(out).toEqual([
      { min: 4, max: 4, avg: 4 },
      null,
      { min: 2, max: 6, avg: 4 },
    ]);
  });

  it("passes short series through one sample per bucket", () => {
    expect(downsampleMinMaxAvg([3, null], 10)).toEqual([
      { min: 3, max: 3, avg: 3 },
      null,
    ]);
  });

  it("returns nothing for zero buckets", () => {
    expect(downsampleMinMaxAvg([1, 2], 0)).toEqual([]);
  });
});

describe("niceCeiling", () => {
  it.each([
    [37, 40],
    [40, 40],
    [41, 50],
    [14.8, 20],
    [5.5, 6],
    [72, 80],
    [81, 100],
    [0.33, 1],
    [1200, 2000],
  ])("snaps %d up to %d", (v, want) => {
    expect(niceCeiling(v)).toBe(want);
  });

  it("uses the floor for idle or missing data", () => {
    expect(niceCeiling(0)).toBe(1);
    expect(niceCeiling(Number.NaN, 5)).toBe(5);
    expect(niceCeiling(0.03, 0.1)).toBe(0.1);
  });

  it("returns exact decimals below 1", () => {
    expect(niceCeiling(0.33, 0.01)).toBe(0.4);
  });

  it("steps down the ladder", () => {
    expect(stepBelow(40)).toBe(20);
    expect(stepBelow(50)).toBe(40);
    expect(stepBelow(10)).toBe(8);
    expect(stepBelow(100)).toBe(80);
    expect(stepBelow(1)).toBe(0);
  });
});

describe("nextCeiling", () => {
  // Drive the clock explicitly, one call per tick, like the 1 Hz sampler.
  function run(
    start: CeilingState | null,
    samples: readonly [ms: number, max: number | null][]
  ): CeilingState {
    let s = start;
    for (const [ms, max] of samples) s = nextCeiling(s, max, ms);
    if (s === null) throw new Error("no samples");
    return s;
  }
  const ticks = (from: number, to: number, max: number | null) =>
    Array.from(
      { length: to - from + 1 },
      (_, i) => [(from + i) * 1000, max] as [number, number | null]
    );

  it("starts at the nice ceiling of the first window", () => {
    expect(nextCeiling(null, 37, 0).ceiling).toBe(40);
  });

  it("grows immediately", () => {
    const s = run(null, [
      [0, 37],
      [1000, 55],
    ]);
    expect(s.ceiling).toBe(60);
  });

  it("does not shrink while data sits above the next step down", () => {
    const s = run(null, [[0, 37], ...ticks(1, 300, 21)]);
    expect(s.ceiling).toBe(40);
  });

  it("shrinks only after 60 s below the next step down", () => {
    const start = run(null, [[0, 37]]);
    const at59 = run(start, ticks(1, 60, 12));
    expect(at59.ceiling).toBe(40);
    const at60 = nextCeiling(at59, 12, 61_000);
    expect(at60.ceiling).toBe(20);
  });

  it("snaps the shrink to the peak seen while below, not the last value", () => {
    const start = run(null, [[0, 37]]);
    const s = run(start, [
      ...ticks(1, 10, 3),
      [11_000, 18],
      ...ticks(12, 61, 3),
    ]);
    expect(s.ceiling).toBe(20);
  });

  it("restarts the shrink timer when data climbs back", () => {
    const start = run(null, [[0, 37]]);
    const s = run(start, [
      ...ticks(1, 50, 12),
      [51_000, 30],
      ...ticks(52, 100, 12),
    ]);
    // Below again from 52 s, so it shrinks at 112 s, not 61 s.
    expect(s.ceiling).toBe(40);
    expect(run(s, ticks(101, 111, 12)).ceiling).toBe(40);
    expect(run(s, ticks(101, 112, 12)).ceiling).toBe(20);
  });

  it("tolerates a skipped tick", () => {
    const start = run(null, [[0, 37]]);
    const s = run(start, [
      ...ticks(1, 30, 12),
      // 31 s missing: the 2 s gap is under maxTickGapMs.
      ...ticks(32, 61, 12),
    ]);
    expect(s.ceiling).toBe(20);
  });

  it("does not count a sleep as time below", () => {
    const start = run(null, [[0, 37], ...ticks(1, 5, 12)]);
    // Lid closed for an hour; first tick after wake is still below.
    const woke = nextCeiling(start, 12, 3_605_000);
    expect(woke.ceiling).toBe(40);
    expect(run(woke, ticks(3606, 3664, 12)).ceiling).toBe(40);
    expect(run(woke, ticks(3606, 3665, 12)).ceiling).toBe(20);
  });

  it("restarts the timer when the clock moves backwards", () => {
    const start = run(null, [[0, 37], ...ticks(1, 59, 12)]);
    expect(nextCeiling(start, 12, 30_000).ceiling).toBe(40);
  });

  it("holds the ceiling through an empty window", () => {
    const start = run(null, [[0, 37], ...ticks(1, 40, 12)]);
    const s = run(start, [...ticks(41, 45, null), ...ticks(46, 90, 12)]);
    expect(s.ceiling).toBe(40);
  });
});

describe("heatmapAlpha", () => {
  it("follows 0.06 + v/vmax * 0.89", () => {
    expect(heatmapAlpha(0, 100)).toBeCloseTo(0.06);
    expect(heatmapAlpha(50, 100)).toBeCloseTo(0.505);
    expect(heatmapAlpha(100, 100)).toBeCloseTo(0.95);
  });

  it("clamps at 0.95 for the 30-day heatmap's vmax of 80", () => {
    expect(heatmapAlpha(71, 80)).toBeCloseTo(0.06 + (71 / 80) * 0.89);
    expect(heatmapAlpha(120, 80)).toBeCloseTo(0.95);
    expect(heatmapAlpha(-5, 80)).toBeCloseTo(0.06);
  });

  it("returns null for a missing sample, not the faintest tint", () => {
    expect(heatmapAlpha(null, 100)).toBeNull();
    expect(heatmapAlpha(Number.NaN, 100)).toBeNull();
  });
});

describe("ringArcs", () => {
  it("reproduces the Overview CPU ring (User 12.4%, System 5.6%)", () => {
    const { circumference, arcs } = ringArcs(30, [0.124, 0.056]);
    expect(circumference).toBeCloseTo(188.5, 1);
    expect(arcs).toEqual([
      { length: 23.4, dasharray: "23.4 188.5", dashoffset: 0 },
      { length: 10.6, dasharray: "10.6 188.5", dashoffset: -23.4 },
    ]);
  });

  it("matches the P-cluster ring at r=48", () => {
    const { arcs } = ringArcs(48, [3.2 / 4.51]);
    expect(arcs[0]?.dasharray).toBe("214.0 301.6");
  });

  it("clamps segments so they never pass the start", () => {
    const { arcs } = ringArcs(30, [0.8, 0.5, 0.1]);
    expect(arcs.map((a) => a.length)).toEqual([150.8, 37.7, 0]);
    expect(arcs[1]?.dashoffset).toBe(-150.8);
  });

  it("clamps out-of-range and non-finite fractions", () => {
    const { arcs } = ringArcs(30, [-0.2, Number.NaN, 1.5]);
    expect(arcs.map((a) => a.length)).toEqual([0, 0, 188.5]);
  });
});
