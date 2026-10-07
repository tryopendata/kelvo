import {
  CHART_WINDOW_MS,
  CHART_WINDOWS,
  effectiveWindow,
  scaledWindowMs,
  slowestIntervalMs,
  windowAllowed,
  windowLabel,
  windowWords,
} from "./live-window";

describe("scaledWindowMs", () => {
  it("keeps the 1 s sample count at slower intervals", () => {
    expect(scaledWindowMs(60_000, 1000)).toBe(60_000);
    expect(scaledWindowMs(60_000, 500)).toBe(60_000);
    expect(scaledWindowMs(60_000, 5000)).toBe(300_000);
    // The popover at 30 s shows the last 30 minutes.
    expect(scaledWindowMs(60_000, 30_000)).toBe(1_800_000);
    expect(scaledWindowMs(48_000, 30_000)).toBe(1_440_000);
  });

  it("never asks for more than the one-hour ring", () => {
    expect(scaledWindowMs(60_000, 60_000)).toBe(3_600_000);
    expect(scaledWindowMs(600_000, 60_000)).toBe(3_600_000);
  });
});

describe("windowAllowed", () => {
  it("drops windows with under 10 samples, never the hour", () => {
    expect(windowAllowed(60_000, 5000)).toBe(true);
    expect(windowAllowed(60_000, 10_000)).toBe(false);
    expect(windowAllowed(300_000, 30_000)).toBe(true);
    expect(windowAllowed(900_000, 60_000)).toBe(true);
    expect(windowAllowed(3_600_000, 600_000)).toBe(true);
  });
});

describe("slowestIntervalMs", () => {
  const sampling = {
    interval_ms: 1000,
    slow_on_battery: false,
    performance_mode: false,
  };

  it("doubles on battery when slow-down is on, capped at 60 s", () => {
    expect(slowestIntervalMs({ ...sampling, slow_on_battery: true })).toBe(
      2000
    );
    expect(slowestIntervalMs(sampling)).toBe(1000);
    expect(
      slowestIntervalMs({
        ...sampling,
        interval_ms: 60_000,
        slow_on_battery: true,
      })
    ).toBe(60_000);
  });

  it("doubles in Performance mode and in Low Power Mode", () => {
    expect(slowestIntervalMs({ ...sampling, performance_mode: true })).toBe(
      2000
    );
    expect(slowestIntervalMs(sampling, true)).toBe(2000);
  });
});

describe("window labels", () => {
  it("names seconds, minutes and the hour", () => {
    expect(windowLabel(60_000)).toBe("60s");
    expect(windowLabel(1_800_000)).toBe("30m");
    expect(windowLabel(3_600_000)).toBe("1h");
    expect(windowWords(48_000)).toBe("48 seconds");
    expect(windowWords(1_800_000)).toBe("30 minutes");
    expect(windowWords(3_600_000)).toBe("hour");
  });
});

describe("effectiveWindow", () => {
  it("keeps the saved window when it holds enough samples", () => {
    for (const w of CHART_WINDOWS) expect(effectiveWindow(w, 1000)).toBe(w);
    expect(effectiveWindow("5m", 30_000)).toBe("5m");
  });

  it("shows the shortest allowed window when the saved one is too short", () => {
    // 5m at 60 s is 5 samples.
    expect(effectiveWindow("5m", 60_000)).toBe("15m");
    expect(effectiveWindow("15m", 60_000)).toBe("15m");
    expect(effectiveWindow("1h", 60_000)).toBe("1h");
  });

  it("returns to the saved window when the interval speeds back up", () => {
    expect(effectiveWindow("5m", 60_000)).toBe("15m");
    expect(effectiveWindow("5m", 1000)).toBe("5m");
  });

  it("lists the windows shortest first, all inside the ring", () => {
    expect(CHART_WINDOWS).toEqual(["5m", "15m", "30m", "1h"]);
    expect(CHART_WINDOWS.map((w) => CHART_WINDOW_MS[w])).toEqual([
      300_000, 900_000, 1_800_000, 3_600_000,
    ]);
  });
});
