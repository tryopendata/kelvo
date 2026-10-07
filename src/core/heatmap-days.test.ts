import { heatmapDays, localDateIso, localHourStarts } from "./heatmap-days";

const HOUR = 3_600_000;
const utc = (iso: string) => Date.parse(iso);

describe("heatmap days in New York", () => {
  // Node re-reads TZ when it is assigned, so `Date` follows it from here on.
  const previous = process.env.TZ;
  beforeAll(() => {
    process.env.TZ = "America/New_York";
  });
  afterAll(() => {
    process.env.TZ = previous;
  });

  it("an ordinary day is 24 one-hour cells from local midnight", () => {
    const starts = localHourStarts(2026, 9, 4);
    expect(starts).toHaveLength(25);
    expect(starts[0]).toBe(utc("2026-10-04T04:00:00Z"));
    expect(starts[24]).toBe(utc("2026-10-05T04:00:00Z"));
    for (let h = 0; h < 24; h++) {
      expect((starts[h + 1] as number) - (starts[h] as number)).toBe(HOUR);
    }
  });

  it("spring forward: 23 hours, local 02:00 is an empty cell", () => {
    const starts = localHourStarts(2026, 2, 8);
    expect(starts[0]).toBe(utc("2026-03-08T05:00:00Z"));
    expect(starts[1]).toBe(utc("2026-03-08T06:00:00Z"));
    expect(starts[2]).toBe(utc("2026-03-08T07:00:00Z"));
    expect(starts[3]).toBe(utc("2026-03-08T07:00:00Z"));
    expect(starts[24]).toBe(utc("2026-03-09T04:00:00Z"));
    expect((starts[24] as number) - (starts[0] as number)).toBe(23 * HOUR);
  });

  it("fall back: 25 hours, local 01:00 is a two-hour cell", () => {
    const starts = localHourStarts(2026, 10, 1);
    expect(starts[0]).toBe(utc("2026-11-01T04:00:00Z"));
    expect(starts[1]).toBe(utc("2026-11-01T05:00:00Z"));
    expect(starts[2]).toBe(utc("2026-11-01T07:00:00Z"));
    expect(starts[24]).toBe(utc("2026-11-02T05:00:00Z"));
    expect((starts[24] as number) - (starts[0] as number)).toBe(25 * HOUR);
  });

  it("30 days end with today's, oldest first, without gaps or overlaps", () => {
    const now = new Date(2026, 10, 15, 14, 30);
    const days = heatmapDays(now);
    expect(days).toHaveLength(30);
    expect(days[29]?.date).toBe("2026-11-15");
    expect(days[0]?.date).toBe("2026-10-17");
    expect(days.map((d) => d.date)).toContain("2026-11-01");
    for (let i = 1; i < days.length; i++) {
      expect(days[i]?.hour_starts[0]).toBe(days[i - 1]?.hour_starts[24]);
    }
    expect(localDateIso(now)).toBe("2026-11-15");
  });
});

/** 25 boundaries, never decreasing, and the cell lengths in minutes. */
function cellMinutes(starts: number[]): number[] {
  expect(starts).toHaveLength(25);
  const out: number[] = [];
  for (let h = 0; h < 24; h++) {
    const len = (starts[h + 1] as number) - (starts[h] as number);
    expect(len).toBeGreaterThanOrEqual(0);
    out.push(len / 60_000);
  }
  return out;
}

describe("heatmap days where midnight or a half hour goes missing", () => {
  const previous = process.env.TZ;
  afterEach(() => {
    process.env.TZ = previous;
  });

  it("Santiago: the spring day has no midnight, so 00:00 is the empty cell", () => {
    process.env.TZ = "America/Santiago";
    const days = heatmapDays(new Date(2026, 8, 6, 12), 2);
    const [before, spring] = days;
    if (!before || !spring) throw new Error("no days");
    expect(spring.date).toBe("2026-09-06");
    // The day before ends where the spring day starts: no gap, no overlap.
    expect(spring.hour_starts[0]).toBe(before.hour_starts[24]);
    expect(cellMinutes(before.hour_starts).every((m) => m === 60)).toBe(true);
    const spr = cellMinutes(spring.hour_starts);
    expect(spr[0]).toBe(0);
    expect(spr.slice(1).every((m) => m === 60)).toBe(true);
    expect(spring.hour_starts[0]).toBe(utc("2026-09-06T04:00:00Z"));
  });

  it("Lord Howe: a 30-minute shift makes a half-hour cell, then a 90-minute one", () => {
    process.env.TZ = "Australia/Lord_Howe";
    const days = heatmapDays(new Date(2026, 9, 4, 12), 2);
    expect(days[1]?.date).toBe("2026-10-04");
    expect(days[1]?.hour_starts[0]).toBe(days[0]?.hour_starts[24]);
    const spring = cellMinutes(days[1]?.hour_starts ?? []);
    expect(spring[2]).toBe(30);
    expect(spring.filter((m) => m !== 60)).toEqual([30]);

    const fall = cellMinutes(localHourStarts(2026, 3, 5));
    expect(fall[1]).toBe(90);
    expect(fall.filter((m) => m !== 60)).toEqual([90]);
  });
});
