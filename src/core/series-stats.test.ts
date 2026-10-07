import { describe, expect, it } from "vitest";
import { initialHostLive, reduceLive, seriesWindow } from "./live-state";
import { residencyRows } from "./residency";
import { seriesKey } from "./series-key";
import { bucketAverages, heatmapBucketMs, seriesStats } from "./series-stats";

const A = "a";
const B = "b{x=1}";

/** A host with rows at the given timestamps; `rows[i]` is `[a, b]`. */
function hostWith(start: number, rows: (number | null)[][], intervalMs = 1000) {
  let s = initialHostLive("h");
  s = reduceLive(s, {
    kind: "layout",
    kinds: [],
    layout_no: 1,
    series: [seriesKey("a"), seriesKey("b", { x: "1" })],
  });
  s = reduceLive(s, {
    kind: "backfill",
    holds_ms: [],
    layout_no: 1,
    timeline: 0,
    start_ms: start,
    interval_ms: intervalMs,
    rows,
  });
  return s;
}

describe("seriesStats", () => {
  it("averages only measured values inside the window", () => {
    const s = hostWith(0, [
      [100, 1],
      [10, null],
      [20, 3],
      [null, 5],
    ]);
    // last row at 3000; a 2.5 s window keeps rows at 1000, 2000, 3000.
    const stats = seriesStats(s, [A, B, "missing"], 2500);
    expect(stats[A]).toEqual({ avg: 15, min: 10, max: 20, count: 2 });
    expect(stats[B]).toEqual({ avg: 4, min: 3, max: 5, count: 2 });
    expect(stats.missing).toBeNull();
  });

  it("is null for a key with no value in the window, never 0", () => {
    const s = hostWith(0, [
      [1, null],
      [2, null],
    ]);
    expect(seriesStats(s, [B], 60_000)[B]).toBeNull();
  });
});

describe("statistics on the display grid (D-092)", () => {
  /** `a` a gauge and `b` a mean, both sampled every 4 s and held 10 s. */
  function slowHost() {
    let s = initialHostLive("h");
    s = reduceLive(s, {
      kind: "layout",
      kinds: ["gauge", "mean"],
      layout_no: 1,
      series: [seriesKey("a"), seriesKey("b", { x: "1" })],
    });
    return reduceLive(s, {
      kind: "backfill",
      holds_ms: [10_000, 10_000],
      layout_no: 1,
      timeline: 0,
      start_ms: 0,
      interval_ms: 1000,
      // Samples at 0 s, 4 s and 8 s.
      rows: Array.from({ length: 9 }, (_, i) =>
        i % 4 === 0 ? [i, i] : [null, null]
      ),
    });
  }

  it("weighs a mean by its span and averages a gauge over its line", () => {
    const s = slowHost();
    const stats = seriesStats(s, [A, B], 9000);
    // The gauge's line 0..8 through every slot: mean 4.
    expect(stats[A]).toEqual({ avg: 4, min: 0, max: 8, count: 9 });
    // The mean fills the slots it covers: 0, 4 x4, 8 x4, so (16 + 32) / 9.
    expect(stats[B]?.avg).toBeCloseTo(48 / 9, 9);
    expect(stats[B]?.count).toBe(9);
  });

  it("buckets the same grid, and agrees with seriesWindow", () => {
    const s = slowHost();
    const out = bucketAverages(s, [B], 3000, 0, 2);
    // Slots 0..2: 0, 4, 4; 3..5: 4, 4, 8; 6..8: 8, 8, 8.
    expect(out[B]).toEqual([8 / 3, 16 / 3, 8]);
    expect(seriesWindow(s, B, 9000).values).toEqual([
      0, 4, 4, 4, 4, 8, 8, 8, 8,
    ]);
  });
});

describe("bucketAverages", () => {
  it("averages rows into wall-clock-aligned buckets", () => {
    // Rows at 8 s .. 21 s; 10 s buckets 0 (8, 9), 1 (10..19), 2 (20, 21).
    const rows = Array.from({ length: 14 }, (_, i) => [8 + i, null]);
    const s = hostWith(8000, rows);
    const out = bucketAverages(s, [A, B], 10_000, 0, 2);
    expect(out[A]).toEqual([8.5, 14.5, 20.5]);
    expect(out[B]).toEqual([null, null, null]);
  });

  it("leaves buckets before the first row and inside a hole null", () => {
    let s = hostWith(30_000, [[1, 1]]);
    s = reduceLive(s, {
      kind: "backfill",
      holds_ms: [],
      layout_no: 1,
      timeline: 0,
      start_ms: 60_000,
      interval_ms: 1000,
      rows: [[3, 3]],
    });
    const out = bucketAverages(s, [A], 10_000, 1, 6);
    expect(out[A]).toEqual([null, null, 1, null, null, 3]);
  });
});

describe("residencyRows", () => {
  it("keeps the four biggest states, highest frequency first, idle last", () => {
    const rows = residencyRows({
      4512: 4,
      3864: 9,
      3204: 18,
      2420: 10,
      1924: 0,
      1260: 0,
      idle: 59,
    });
    expect(rows).toEqual([
      { label: "4.51 GHz", pct: 4 },
      { label: "3.86 GHz", pct: 9 },
      { label: "3.20 GHz", pct: 18 },
      { label: "2.42 GHz", pct: 10 },
      { label: "idle", pct: 59 },
    ]);
  });

  it("merges the rest into other when it is worth showing", () => {
    const rows = residencyRows({
      3000: 10,
      2500: 10,
      2000: 10,
      1500: 10,
      1000: 3,
      600: 2,
      idle: 55,
    });
    expect(rows?.map((r) => r.label)).toEqual([
      "3.00 GHz",
      "2.50 GHz",
      "2.00 GHz",
      "1.50 GHz",
      "other",
      "idle",
    ]);
    expect(rows?.find((r) => r.label === "other")?.pct).toBe(5);
  });

  it("does not spend a row on a state that would read 0%", () => {
    const rows = residencyRows({
      2892: 6,
      2160: 11,
      1500: 0.2,
      1020: 42,
      idle: 41,
    });
    expect(rows?.map((r) => r.label)).toEqual([
      "2.89 GHz",
      "2.16 GHz",
      "1.02 GHz",
      "idle",
    ]);
  });

  it("skips states with no value and returns null when none has one", () => {
    expect(residencyRows({ 2000: null, idle: null })).toBeNull();
    expect(residencyRows({ 2000: null, idle: 70 })).toEqual([
      { label: "idle", pct: 70 },
    ]);
  });
});

describe("heatmapBucketMs", () => {
  it("keeps about 60 columns per window", () => {
    expect(heatmapBucketMs(300_000, 1000)).toBe(5000);
    expect(heatmapBucketMs(900_000, 1000)).toBe(15_000);
    expect(heatmapBucketMs(1_800_000, 1000)).toBe(30_000);
    expect(heatmapBucketMs(3_600_000, 1000)).toBe(60_000);
    expect(heatmapBucketMs(300_000, 500)).toBe(5000);
  });

  it("raises the bucket to the first step at or above the interval", () => {
    // 5m at 10 s: a 5 s bucket would be empty every other column.
    expect(heatmapBucketMs(300_000, 10_000)).toBe(15_000);
    expect(heatmapBucketMs(300_000, 2000)).toBe(5000);
    expect(heatmapBucketMs(900_000, 30_000)).toBe(30_000);
    expect(heatmapBucketMs(900_000, 60_000)).toBe(60_000);
    expect(heatmapBucketMs(3_600_000, 60_000)).toBe(60_000);
  });
});
