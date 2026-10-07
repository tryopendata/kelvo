import { initialHostLive, reduceLive } from "@core/live-state";
import { seriesKey } from "@core/series-key";
import { intervalLabel, selfCpuAverage } from "./overhead";

/**
 * `self.cpu` (a mean) readings `stepMs` apart, each held `holdMs`, with the
 * sampling setup last changed at `statusSinceMs` (by default long before
 * these rows).
 */
function state(
  values: (number | null)[],
  stepMs = 1000,
  {
    holdMs = 0,
    statusSinceMs = -3_600_000,
  }: { holdMs?: number; statusSinceMs?: number | null } = {}
) {
  let s = initialHostLive("h");
  s = reduceLive(s, {
    kind: "layout",
    layout_no: 1,
    series: [seriesKey("cpu.total", {}), seriesKey("self.cpu", {})],
    kinds: ["gauge", "mean"],
  });
  if (values.length === 0) return s;
  s = reduceLive(s, {
    kind: "backfill",
    holds_ms: [2500, holdMs],
    layout_no: 1,
    timeline: 0,
    start_ms: 0,
    interval_ms: stepMs,
    rows: values.map((v) => [50, v]),
  });
  return { ...s, statusSinceMs };
}

describe("selfCpuAverage", () => {
  it("averages the readings and skips ticks without one", () => {
    expect(selfCpuAverage(state([0.3, null, null, 0.5, null]))).toBe(0.4);
  });

  it("weighs each reading by the seconds it averages", () => {
    // 0.6 covers the three seconds since 0.2: (0.2 + 3 x 0.6) / 4 = 0.5,
    // where a plain mean of the readings would be 0.4.
    expect(
      selfCpuAverage(state([0.2, null, null, 0.6], 1000, { holdMs: 25_000 }))
    ).toBe(0.5);
  });

  it("uses only the last ten minutes", () => {
    // One reading per minute for 20 minutes: 9.0 for the first ten, 0.4 after.
    const values = Array.from({ length: 21 }, (_, i) => (i < 10 ? 9 : 0.4));
    expect(selfCpuAverage(state(values, 60_000))).toBe(0.4);
  });

  it("is null with no reading, so the sentence hides instead of saying 0%", () => {
    expect(selfCpuAverage(state([null, null]))).toBeNull();
    expect(selfCpuAverage(state([]))).toBeNull();
  });

  it("counts only readings since the sampling setup changed", () => {
    // 4.6% at the old interval, then 0.2% after the change at 5 min.
    const values = Array.from({ length: 10 }, (_, i) => (i < 5 ? 4.6 : 0.2));
    // The reading at the change still spans the old setup and is skipped.
    values[5] = 4.6;
    expect(
      selfCpuAverage(state(values, 60_000, { statusSinceMs: 5 * 60_000 }))
    ).toBe(0.2);
  });

  it("skips the reading that spans a change just before the window", () => {
    // A reading every 70 s; the window starts at 240 s. The setup changed at
    // 230 s, before the window, so the 280 s reading (the window's first)
    // still spans the old setup.
    const values = Array.from({ length: 13 }, (_, i) => (i <= 4 ? 9 : 0.4));
    expect(
      selfCpuAverage(state(values, 70_000, { statusSinceMs: 230_000 }))
    ).toBe(0.4);
  });

  it("is null while paused: nothing is being measured", () => {
    const paused = {
      ...state([0.3, 0.4, 0.5]),
      status: {
        interval_ms: 1000,
        frame_period_ms: 1000,
        paused: true,
        display_idle: false,
        on_battery: false,
        performance: "off" as const,
        power_source: "adapter" as const,
        primary_iface: null,
      },
    };
    expect(selfCpuAverage(paused)).toBeNull();
  });

  it("is measuring until two readings follow the change", () => {
    const values = [4.6, 4.6, 4.6, 0.2];
    // Change at the third reading: it is skipped, one reading follows.
    expect(
      selfCpuAverage(state(values, 10_000, { statusSinceMs: 20_000 }))
    ).toBe("measuring");
    // No frame since the change yet.
    expect(selfCpuAverage(state(values, 10_000, { statusSinceMs: null }))).toBe(
      "measuring"
    );
  });
});

describe("intervalLabel", () => {
  it("matches the settings pills", () => {
    expect([500, 1000, 2000, 5000].map(intervalLabel)).toEqual([
      "0.5s",
      "1s",
      "2s",
      "5s",
    ]);
  });
});
