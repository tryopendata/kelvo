import type { Gap, HistoryPage } from "@core/generated/bindings";
import { lastWakeMs, pageMax, scaleFrom } from "./history";

const page = (series: { t: number; max: number | null }[][]): HistoryPage =>
  ({
    series: series.map((points) => ({
      key: { metric: "disk.read_total", labels: [] },
      points: points.map((p) => ({ ...p, min: p.max, avg: p.max })),
    })),
    gaps: [],
  }) as unknown as HistoryPage;

describe("pageMax", () => {
  it("takes the largest bucket max", () => {
    expect(
      pageMax(
        page([
          [
            { t: 1, max: 100 },
            { t: 2, max: 300 },
            { t: 3, max: null },
          ],
        ])
      )
    ).toBe(300);
  });

  it("is null when nothing was recorded", () => {
    expect(pageMax(page([[{ t: 1, max: null }]]))).toBeNull();
  });
});

describe("scaleFrom", () => {
  it("never scales below the floor", () => {
    expect(scaleFrom(null, 5)).toBe(5);
    expect(scaleFrom(3, 5)).toBe(5);
    expect(scaleFrom(12, 5)).toBe(12);
  });
});

describe("lastWakeMs", () => {
  const g = (end_ms: number | null, reason: Gap["reason"] = "sleep"): Gap => ({
    start_ms: 0,
    end_ms,
    module: null,
    reason,
  });

  it("is the end of the latest closed sleep gap", () => {
    expect(lastWakeMs([g(10), g(30), g(null), g(50, "paused")])).toBe(30);
    expect(lastWakeMs([])).toBeNull();
  });
});
