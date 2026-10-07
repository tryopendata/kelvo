import {
  approxSize,
  bytesPerMinuteDay,
  daysUnderLimit,
  FILL_TEST,
  fixedBytes,
  projectedHistoryBytes,
  retentionProjection,
  sizeLimitLabel,
} from "./history-projection";

const MB = 1e6;

describe("projectedHistoryBytes", () => {
  it("reproduces the D-076 fill test runs (7 days of minutes, then quarters)", () => {
    for (const run of [FILL_TEST.rolled.small, FILL_TEST.rolled.large]) {
      expect(projectedHistoryBytes(run.days, run.series) / MB).toBeCloseTo(
        run.bytes / MB,
        6
      );
    }
  });

  it("lands on D-057's capped run: 17.4 days all in minutes at 250 series", () => {
    const bytes = fixedBytes(250) + (24_999 / 1440) * bytesPerMinuteDay(250);
    expect(bytes / MB).toBeCloseTo(139.9, 0);
  });

  it("grows with retention and with the series count", () => {
    expect(projectedHistoryBytes(7)).toBeLessThan(projectedHistoryBytes(30));
    expect(projectedHistoryBytes(30, 100)).toBeLessThan(
      projectedHistoryBytes(30, 150)
    );
    // Retention-independent parts (24 h of 10 s, 72 h of snapshots) keep a
    // short retention from projecting near zero.
    expect(projectedHistoryBytes(7, 150) / MB).toBeCloseTo(64.9, 1);
  });

  it("costs days past the first week at 15-minute resolution", () => {
    const week = projectedHistoryBytes(7, 150);
    const minuteDay = week - projectedHistoryBytes(6, 150);
    const quarterDay =
      projectedHistoryBytes(31, 150) - projectedHistoryBytes(30, 150);
    expect(quarterDay * 15).toBeCloseTo(minuteDay, 0);
    // 90 days fits easily under the default 150 MB limit.
    expect(projectedHistoryBytes(90, 150) / MB).toBeCloseTo(82.9, 1);
    expect(projectedHistoryBytes(90, 250) / MB).toBeCloseTo(115.8, 1);
  });
});

describe("daysUnderLimit", () => {
  it("matches D-076's 92 MB cap run at 250 series: the trim reaches into the minutes", () => {
    expect(daysUnderLimit(92 * MB, 250)).toBe(6);
  });

  it("counts quarter days once the week of minutes fits", () => {
    // 100 MB at 250 series: 7 days of minutes and 20 of quarters. Costed
    // all in minutes, the same room would hold 8.
    expect(daysUnderLimit(100 * MB, 250)).toBe(27);
  });

  it("gives more days to a larger limit and fewer to more series", () => {
    expect(daysUnderLimit(300 * MB, 150)).toBeGreaterThan(
      daysUnderLimit(150 * MB, 150)
    );
    expect(daysUnderLimit(150 * MB, 600)).toBeLessThan(
      daysUnderLimit(150 * MB, 250)
    );
  });

  it("never goes under the 24 h floor", () => {
    expect(daysUnderLimit(10 * MB, 1000)).toBe(1);
  });
});

describe("retentionProjection", () => {
  it("is the plain projection while it fits", () => {
    expect(retentionProjection(90, 150 * MB, 150)).toEqual({
      bytes: projectedHistoryBytes(90, 150),
      limitedDays: null,
    });
  });

  it("caps the size at the limit and says how many days fit", () => {
    expect(retentionProjection(30, 92 * MB, 250)).toEqual({
      bytes: 92 * MB,
      limitedDays: 6,
    });
    expect(retentionProjection(90, 150 * MB, 600).limitedDays).toBe(
      daysUnderLimit(150 * MB, 600)
    );
    // A big enough limit lifts it.
    expect(retentionProjection(90, 500 * MB, 600).limitedDays).toBeNull();
  });
});

describe("with this Mac's measured growth", () => {
  const growth = {
    measured_ms: 12 * 3_600_000,
    fixed_bytes: 36 * MB,
    minute_day_bytes: 2 * MB,
  };

  it("replaces the fill tests' numbers, whatever the series count", () => {
    // 36 MB, then 7 minute days and 23 at a fifteenth: 36 + 2 × (7 + 23/15).
    const bytes = retentionProjection(30, 150 * MB, 600, growth).bytes;
    expect(bytes / MB).toBeCloseTo(53.07, 2);
  });

  it("sets how many days a tight limit keeps", () => {
    // 95% of 50 MB less 36 MB fixed leaves 11.5 MB: five minute days.
    expect(retentionProjection(30, 50 * MB, 150, growth)).toEqual({
      bytes: 50 * MB,
      limitedDays: 5,
    });
  });
});

describe("labels", () => {
  it("rounds estimates to 10 MB and shows GB from 1000 MB", () => {
    expect(approxSize(69.9 * MB)).toBe("70 MB");
    expect(approxSize(2 * MB)).toBe("10 MB");
    expect(approxSize(1_234 * MB)).toBe("1.2 GB");
    expect(sizeLimitLabel(150)).toBe("150 MB");
    expect(sizeLimitLabel(1000)).toBe("1 GB");
  });
});
