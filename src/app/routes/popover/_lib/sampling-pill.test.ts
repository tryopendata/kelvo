import { samplingPill } from "./sampling-pill";

describe("samplingPill", () => {
  it("shows the effective interval while live", () => {
    expect(
      samplingPill({ intervalMs: 2000, paused: false, stale: false })
    ).toEqual({ state: "live", label: "2s" });
    expect(
      samplingPill({ intervalMs: 500, paused: false, stale: false })
    ).toEqual({ state: "live", label: "0.5s" });
  });

  it("puts paused before stale", () => {
    expect(
      samplingPill({ intervalMs: 1000, paused: true, stale: true })
    ).toEqual({ state: "paused", label: "Paused" });
    expect(
      samplingPill({ intervalMs: 1000, paused: false, stale: true })
    ).toEqual({ state: "stale", label: "Stale" });
  });

  it("says Reconnecting while the channel resubscribes", () => {
    expect(
      samplingPill({
        intervalMs: 1000,
        paused: false,
        stale: true,
        connection: "reconnecting",
      })
    ).toEqual({ state: "stale", label: "Reconnecting" });
  });

  it("shows the frame period with perf in Performance mode", () => {
    for (const performance of ["setting", "low_power_mode"] as const) {
      expect(
        samplingPill({
          intervalMs: 1000,
          framePeriodMs: 2000,
          paused: false,
          stale: false,
          performance,
        })
      ).toEqual({ state: "live", label: "2s · perf" });
    }
    expect(
      samplingPill({
        intervalMs: 1000,
        framePeriodMs: 1000,
        paused: false,
        stale: false,
        performance: "off",
      })
    ).toEqual({ state: "live", label: "1s" });
  });
});
