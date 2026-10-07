import {
  alignColumns,
  type Bucket,
  bucketAt,
  combineSeries,
  gapEdgeIndices,
  peakOf,
} from "./buckets";

const S10 = 10_000;
const b = (t: number, avg: number, min = avg, max = avg): Bucket => ({
  t,
  avg,
  min,
  max,
});

describe("combineSeries", () => {
  it("sums interfaces bucket by bucket and keeps a bucket only one has", () => {
    const out = combineSeries(
      [
        { points: [b(0, 10, 8, 12), b(10_000, 10, 8, 12)] },
        { points: [b(0, 1, 0, 2)] },
      ],
      "sum"
    );
    expect(out).toEqual([b(0, 11, 8, 14), b(10_000, 10, 8, 12)]);
  });

  it("takes the max for fans", () => {
    const out = combineSeries(
      [
        { points: [b(0, 1800, 1700, 1900)] },
        { points: [b(0, 1850, 1600, 2000)] },
      ],
      "max"
    );
    expect(out).toEqual([b(0, 1850, 1700, 2000)]);
  });
});

describe("alignColumns", () => {
  it("puts a null slot where buckets are further apart than the hold", () => {
    const { x, series } = alignColumns(
      [[b(0, 1), b(10_000, 2), b(40_000, 3)]],
      [S10],
      S10
    );
    expect(x).toEqual([0, 10_000, 20_000, 40_000]);
    expect(series[0]?.avg).toEqual([1, 2, null, 3]);
  });

  it("joins a 60 s series at S10: disk.used is one line, not dots", () => {
    // `disk.used` is sampled every 60 s; Rust's hold is 2.5 periods.
    const used = [b(0, 388), b(60_000, 388), b(120_000, 389)];
    const cpu = Array.from({ length: 13 }, (_, i) => b(i * S10, 20));
    const { x, series } = alignColumns([cpu, used], [S10, 150_000], S10);
    expect(x).toHaveLength(13);
    const col = series[1]?.avg ?? [];
    // Measured slots, and slots between them that uPlot spans.
    expect(col[0]).toBe(388);
    expect(col.slice(1, 6)).toEqual(Array(5).fill(undefined));
    expect(col[6]).toBe(388);
    expect(col[12]).toBe(389);
    expect(col.includes(null)).toBe(false);
  });

  it("breaks a slow series further apart than its hold", () => {
    const used = [b(0, 388), b(240_000, 389)];
    const { x, series } = alignColumns([used], [150_000], S10);
    expect(x).toEqual([0, 10_000, 240_000]);
    expect(series[0]?.avg).toEqual([388, null, 389]);
  });

  it("breaks at a gap start and drops buckets inside the gap", () => {
    const { x, series } = alignColumns(
      [[b(0, 1), b(10_000, 2), b(20_000, 9), b(30_000, 3)]],
      [150_000],
      S10,
      [{ fromMs: 15_000, toMs: 30_000 }]
    );
    expect(x).toEqual([0, 10_000, 15_000, 30_000]);
    expect(series[0]?.avg).toEqual([1, 2, null, 3]);
  });

  it("aligns two series on one x column, null outside a series", () => {
    const { x, series } = alignColumns(
      [[b(0, 1), b(10_000, 2)], [b(10_000, 5)]],
      [S10, S10],
      S10
    );
    expect(x).toEqual([0, 10_000]);
    expect(series[1]?.avg).toEqual([null, 5]);
  });
});

describe("bucketAt (crosshair lookup)", () => {
  const buckets = [b(0, 1), b(10_000, 2), b(40_000, 3)];

  it("finds the bucket containing the time", () => {
    expect(bucketAt(buckets, 0, S10)?.avg).toBe(1);
    expect(bucketAt(buckets, 19_999, S10)?.avg).toBe(2);
    expect(bucketAt(buckets, 45_000, S10)?.avg).toBe(3);
  });

  it("returns null in a hole and outside the series", () => {
    expect(bucketAt(buckets, 20_000, S10)).toBeNull();
    expect(bucketAt(buckets, 35_000, S10)).toBeNull();
    expect(bucketAt(buckets, -1, S10)).toBeNull();
    expect(bucketAt(buckets, 50_000, S10)).toBeNull();
    expect(bucketAt([], 0, S10)).toBeNull();
  });
});

describe("gapEdgeIndices", () => {
  it("marks the last slot before a gap and the first after it", () => {
    const x = [0, 10_000, 20_000, 60_000, 70_000];
    const avg = [1, 2, null, 3, 4];
    expect(
      gapEdgeIndices(x, avg, [{ fromMs: 20_000, toMs: 60_000 }], S10, S10)
    ).toEqual([1, 3]);
  });

  it("reaches as far as the series' hold, not one bucket", () => {
    // A minute-sampled series on 10 s buckets: its last point is 50 s
    // before the band, its first 40 s after it.
    const x = [0, 60_000, 200_000, 260_000];
    const avg = [1, 2, 3, 4];
    const gap = [{ fromMs: 120_000, toMs: 160_000 }];
    expect(gapEdgeIndices(x, avg, gap, S10, 60_000)).toEqual([1, 2]);
    expect(gapEdgeIndices(x, avg, gap, S10, S10)).toEqual([]);
  });
});

describe("peakOf", () => {
  it("returns the highest max and when it was", () => {
    expect(
      peakOf([b(0, 10, 5, 20), b(60_000, 30, 25, 71), b(120_000, 9)])
    ).toEqual({ value: 71, tMs: 60_000 });
    expect(peakOf([])).toBeNull();
  });
});
