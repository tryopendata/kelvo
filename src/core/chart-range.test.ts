import {
  chartRange,
  chartRangeLabel,
  customRange,
  dateInput,
  MAX_RANGE_MS,
  parseDateInput,
  validChartRange,
} from "./chart-range";

describe("chart ranges", () => {
  it("preserves arbitrary durations instead of rounding to an hour", () => {
    expect(chartRange({ relative: 5_400_000 })).toEqual({
      windowMs: 5_400_000,
      endMs: null,
    });
    expect(chartRangeLabel({ relative: 5_400_000 })).toBe("90m");
    expect(chartRangeLabel({ relative: 172_800_000 })).toBe("2d");
  });
  it.each([0, -1, NaN, Infinity, 59_999, MAX_RANGE_MS + 1])(
    "rejects duration %s",
    (relative) => {
      expect(validChartRange({ relative })).toBe(false);
    }
  );
  it("rejects reversed dates, unsafe timestamps and pre-epoch dates", () => {
    for (const absolute of [
      { from_ms: 90_000, to_ms: 60_000 },
      { from_ms: -1, to_ms: 90_000 },
      {
        from_ms: Number.MAX_SAFE_INTEGER - 60_000,
        to_ms: Number.MAX_SAFE_INTEGER + 1,
      },
    ]) {
      expect(validChartRange({ absolute })).toBe(false);
    }
  });
  it("validates real local dates, including leap years", () => {
    expect(parseDateInput("2026-02-30 12:00")).toBeNull();
    expect(parseDateInput("2026-01-12 25:00")).toBeNull();
    expect(parseDateInput("1/12/2026")).toBeNull();
    const leap = parseDateInput("2024-02-29 12:00");
    expect(leap).not.toBeNull();
    expect(dateInput(leap as number)).toBe("2024-02-29 12:00");
  });
  it("accepts a custom shortcut duration and rejects future dates", () => {
    const draft = {
      mode: "relative",
      amount: "5",
      unitMs: 60_000,
      from: "2026-01-01 10:00",
      to: "2026-01-01 12:00",
      now: new Date("2026-01-01T11:00").getTime(),
      intervalMs: 1000,
    };
    expect(customRange(draft)).toEqual({ value: "5m" });
    expect(customRange({ ...draft, mode: "absolute" })).toEqual({
      error: "The end time must be now or earlier.",
    });
    expect(customRange({ ...draft, intervalMs: 60_000 })).toHaveProperty(
      "error"
    );
  });
});
