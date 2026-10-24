import type { HistoryPage } from "./generated/bindings";
import { historyGrid } from "./history-grid";

it("keeps exact series labels, missing buckets and explicit gaps on the history grid", () => {
  const point = (t: number, avg: number) => ({ t, avg, min: avg, max: avg });
  const page: HistoryPage = {
    tier: "s10",
    bucket_ms: 10_000,
    series: [
      {
        key: { metric: "cpu.load", labels: [["core", "P0"]] },
        hold_ms: 10_000,
        points: [point(10_000, 20), point(20_000, 90), point(40_000, 30)],
      },
      {
        key: { metric: "cpu.load", labels: [["core", "P1"]] },
        hold_ms: 10_000,
        points: [point(10_000, 99)],
      },
    ],
    gaps: [{ start_ms: 25_000, end_ms: 28_000, reason: "sleep", module: null }],
  };
  expect(
    historyGrid(page, ["cpu.load{core=P0}"], 10_000, 50_000, false).values[
      "cpu.load{core=P0}"
    ]
  ).toEqual([20, 90, null, 30]);
  const grid = historyGrid(
    page,
    ["cpu.load{core=P0}", "cpu.total"],
    10_000,
    50_000
  );
  expect(grid).toEqual({
    values: {
      "cpu.load{core=P0}": [20, null, null, 30],
      "cpu.total": [null, null, null, null],
    },
    bucketMs: 10_000,
    endMs: 40_000,
  });
});

it("preserves Rust's bucket origin when merged widths do not align to Unix zero", () => {
  const page: HistoryPage = {
    tier: "s10",
    bucket_ms: 30_000,
    gaps: [],
    series: [
      {
        key: { metric: "cpu.total", labels: [] },
        hold_ms: 30_000,
        points: [{ t: 40_000, min: 1, max: 1, avg: 1 }],
      },
    ],
  };
  expect(historyGrid(page, ["cpu.total"], 12_000, 72_000)).toEqual({
    values: { "cpu.total": [null, 1, null] },
    endMs: 70_000,
    bucketMs: 30_000,
  });
});
