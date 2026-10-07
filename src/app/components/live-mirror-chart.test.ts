import { brushBucketMs } from "@core/brush";
import { CHART_WINDOW_MS, CHART_WINDOWS } from "@core/live-window";
import { mirrorBuckets } from "./live-mirror-chart";
import { slotAt } from "./mirror-hover";

describe("mirror chart buckets (D-091)", () => {
  it("30m at 1 s: 15 s bars, 30 s when brushable", () => {
    expect(mirrorBuckets(1_800_000, 1000)).toEqual({
      bucketMs: 15_000,
      count: 120,
    });
    expect(brushBucketMs(15_000, 1000)).toBe(30_000);
  });

  it.each([250, 500, 1000, 2000, 5000, 10_000, 30_000, 60_000])(
    "no window draws more than 120 bars at %i ms sampling",
    (intervalMs) => {
      for (const w of CHART_WINDOWS) {
        const plain = mirrorBuckets(CHART_WINDOW_MS[w], intervalMs);
        expect(plain.count).toBeLessThanOrEqual(120);
        // Brushable bars are never narrower, so never more of them.
        expect(
          brushBucketMs(plain.bucketMs, intervalMs)
        ).toBeGreaterThanOrEqual(plain.bucketMs);
      }
    }
  );
});

describe("slotAt", () => {
  const box = { left: 100, width: 600 };
  it("maps the pointer to the bar under it", () => {
    expect(slotAt(100, box, 120)).toBe(0);
    expect(slotAt(104.9, box, 120)).toBe(0);
    expect(slotAt(105, box, 120)).toBe(1);
    expect(slotAt(699.9, box, 120)).toBe(119);
  });
  it("is null off the bars, on either side", () => {
    expect(slotAt(99, box, 120)).toBeNull();
    expect(slotAt(700, box, 120)).toBeNull();
    expect(slotAt(50, { left: 0, width: 0 }, 120)).toBeNull();
  });
});
