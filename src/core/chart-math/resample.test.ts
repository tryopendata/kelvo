import { resampleBuckets } from "@core/chart-math";

const T = 1_800_000_000_000;

describe("resampleBuckets", () => {
  it("gives each slot the bucket its timestamp falls in", () => {
    // Buckets [T, T+10s) = 1, [T+10s, T+20s) = 2; slots every 5 s to T+15s.
    expect(
      resampleBuckets(
        { fromMs: T, bucketMs: 10_000, values: [1, 2] },
        { tEndMs: T + 15_000, intervalMs: 5_000, length: 4 }
      )
    ).toEqual([1, 1, 2, 2]);
  });

  it("is null before the first bucket and after the last", () => {
    expect(
      resampleBuckets(
        { fromMs: T, bucketMs: 10_000, values: [1] },
        { tEndMs: T + 10_000, intervalMs: 5_000, length: 4 }
      )
    ).toEqual([null, 1, 1, null]);
  });

  it("keeps a gap bucket a gap instead of bridging it", () => {
    expect(
      resampleBuckets(
        { fromMs: T, bucketMs: 10_000, values: [1, null, 3] },
        { tEndMs: T + 29_000, intervalMs: 1_000, length: 30 }
      ).filter((_v, i) => i % 10 === 0)
    ).toEqual([1, null, 3]);
  });

  it("is all null with no buckets", () => {
    expect(
      resampleBuckets(
        { fromMs: T, bucketMs: 10_000, values: [] },
        { tEndMs: T, intervalMs: 1_000, length: 3 }
      )
    ).toEqual([null, null, null]);
  });
});
