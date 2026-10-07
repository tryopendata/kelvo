import type { HistoryPage, HistoryPoint } from "@core/generated/bindings";
import { mergeTail, tailRange } from "./history-tail";

const MIN = 60_000;
const pt = (t: number, avg: number): HistoryPoint => ({
  t,
  avg,
  min: avg,
  max: avg,
});

function page(
  points: HistoryPoint[],
  over: Partial<HistoryPage> = {}
): HistoryPage {
  return {
    tier: "m1",
    bucket_ms: MIN,
    series: [
      { key: { metric: "cpu.total", labels: [] }, points, hold_ms: MIN },
    ],
    gaps: [],
    ...over,
  };
}

describe("tailRange", () => {
  it("reads from the slot open at the read through the one open now", () => {
    const p = page([pt(0, 1), pt(MIN, 2)]);
    // Read 30 s into the third minute; now 10 s into the fourth.
    expect(tailRange(p, 2 * MIN + 30_000, 3 * MIN + 10_000)).toEqual({
      fromMs: 2 * MIN,
      toMs: 4 * MIN,
      tier: "m1",
      maxPoints: 2,
    });
  });

  it("keeps the page's slot grid when it is not on multiples of the width", () => {
    // 10-minute slots starting 3 minutes past a multiple of 10.
    const w = 10 * MIN;
    const p = page([pt(3 * MIN, 1)], { bucket_ms: w });
    const r = tailRange(p, 15 * MIN, 24 * MIN);
    expect(r).toEqual({
      fromMs: 13 * MIN,
      toMs: 33 * MIN,
      tier: "m1",
      maxPoints: 2,
    });
  });

  it("is null for a tier it cannot ask for, or no width", () => {
    expect(tailRange(page([], { tier: "m15" }), 0, MIN)).toBeNull();
    expect(tailRange(page([], { bucket_ms: 0 }), 0, MIN)).toBeNull();
  });
});

describe("mergeTail", () => {
  it("replaces the page from the tail's start and drops what left the window", () => {
    const old = page([pt(0, 1), pt(MIN, 2), pt(2 * MIN, 3)], {
      gaps: [
        { start_ms: -5 * MIN, end_ms: -MIN, module: null, reason: "sleep" },
        { start_ms: 0, end_ms: MIN, module: null, reason: "sleep" },
        { start_ms: 2 * MIN, end_ms: null, module: null, reason: "paused" },
      ],
    });
    const tail = page([pt(2 * MIN, 30), pt(3 * MIN, 40)], {
      gaps: [
        { start_ms: 2 * MIN, end_ms: 3 * MIN, module: null, reason: "paused" },
      ],
    });
    const merged = mergeTail(old, tail, 2 * MIN, 30_000);
    expect(merged.series[0]?.points).toEqual([
      pt(MIN, 2),
      pt(2 * MIN, 30),
      pt(3 * MIN, 40),
    ]);
    // The open gap the tail closed is replaced; the one before the window dropped.
    expect(merged.gaps).toEqual([
      { start_ms: 0, end_ms: MIN, module: null, reason: "sleep" },
      { start_ms: 2 * MIN, end_ms: 3 * MIN, module: null, reason: "paused" },
    ]);
  });

  it("adds a series only the tail has", () => {
    const tail = page([pt(MIN, 5)]);
    const other = tail.series[0];
    if (!other) throw new Error("no series");
    tail.series = [{ ...other, key: { metric: "cpu.user", labels: [] } }];
    const merged = mergeTail(page([pt(0, 1)]), tail, MIN, 0);
    expect(merged.series.map((s) => s.key.metric)).toEqual([
      "cpu.total",
      "cpu.user",
    ]);
  });
});
