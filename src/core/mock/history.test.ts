import type { Gap, HistoryRequest } from "@core/generated/bindings";
import { MOCK_HOST_ID, scenarioFlags } from "./fixtures";
import { buildSpecs } from "./generator";
import { mockGaps, mockHistory, mockRecentHistory } from "./history";

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
const NOW = 1_760_000_005_000;
const flags = scenarioFlags([]);
const specs = buildSpecs(flags);
const gaps = mockGaps(flags, NOW);

function request(fromMs: number, maxPoints: number): HistoryRequest {
  return {
    host: MOCK_HOST_ID,
    selectors: [{ metric: "cpu.total", labels: [] }],
    from_ms: fromMs,
    to_ms: NOW,
    tier: "auto",
    max_points: maxPoints,
  };
}

const overlaps = (t: number, w: number, g: Gap) =>
  t + w > g.start_ms && t < (g.end_ms ?? Number.POSITIVE_INFINITY);

describe("mockGaps over 30 days", () => {
  it("has a sleep most nights, none overlapping", () => {
    const month = gaps.filter((g) => g.start_ms >= NOW - 30 * DAY);
    const sleeps = month.filter((g) => g.reason === "sleep");
    // 30 nights; one weekend away covers two of them.
    expect(sleeps.length).toBe(29);
    const sorted = [...month].sort((a, b) => a.start_ms - b.start_ms);
    for (let i = 1; i < sorted.length; i++) {
      const prev = sorted[i - 1] as Gap;
      expect((sorted[i] as Gap).start_ms).toBeGreaterThanOrEqual(
        prev.end_ms ?? Number.POSITIVE_INFINITY
      );
    }
  });

  it("has a few multi-hour holes besides the nights", () => {
    const long = gaps.filter(
      (g) => g.end_ms !== null && g.end_ms - g.start_ms >= 24 * HOUR
    );
    expect(long).toHaveLength(1);
    expect(gaps.find((g) => g.reason === "app_not_running")).toBeDefined();
    expect(gaps.find((g) => g.reason === "paused")).toBeDefined();
  });

  it("keeps a single sleep in the last 24 hours", () => {
    const day = gaps.filter(
      (g) => (g.end_ms ?? NOW) > NOW - DAY && g.start_ms < NOW
    );
    expect(day).toHaveLength(1);
    expect((day[0]?.end_ms ?? 0) - (day[0]?.start_ms ?? 0)).toBe(
      5 * HOUR + 15 * MIN
    );
  });
});

describe("mockHistory over long ranges", () => {
  it("answers 30d from quarters merged to max_points", () => {
    const from = Math.floor((NOW - 30 * DAY) / (30 * MIN)) * 30 * MIN;
    const max = Math.ceil((NOW - from) / (30 * MIN));
    const page = mockHistory(request(from, max), specs, gaps, NOW, 1000);
    expect(page.tier).toBe("m15");
    expect(page.bucket_ms).toBe(30 * MIN);
    const points = page.series[0]?.points ?? [];
    expect(points.length).toBeLessThanOrEqual(max);
    // Nights take out roughly a quarter of the month; the rest is there.
    expect(points.length).toBeGreaterThan(900);
    for (const p of points) expect((p.t - from) % (30 * MIN)).toBe(0);
  });

  it("leaves every slot that touches a gap out, never filled", () => {
    const from = Math.floor((NOW - 30 * DAY) / (30 * MIN)) * 30 * MIN;
    const max = Math.ceil((NOW - from) / (30 * MIN));
    const page = mockHistory(request(from, max), specs, gaps, NOW, 1000);
    const points = page.series[0]?.points ?? [];
    for (const p of points) {
      expect(gaps.some((g) => overlaps(p.t, 30 * MIN, g))).toBe(false);
      expect(p.avg).not.toBeNull();
    }
    expect(page.gaps.length).toBeGreaterThanOrEqual(30);
  });

  it("answers 7d ending now from minutes", () => {
    const from = Math.floor((NOW - 7 * DAY) / (10 * MIN)) * 10 * MIN;
    // One slot per 10 minutes, the open one included.
    const max = Math.ceil((NOW - from) / (10 * MIN));
    const page = mockHistory(request(from, max), specs, gaps, NOW, 1000);
    expect(page.tier).toBe("m1");
    expect(page.bucket_ms).toBe(10 * MIN);
    const points = page.series[0]?.points ?? [];
    expect(points.length).toBeLessThanOrEqual(max);
    for (const p of points) expect((p.t - from) % (10 * MIN)).toBe(0);
  });

  it("widens the min to max envelope with the merged width", () => {
    const meanSpread = (from: number, bucket: number) => {
      const max = Math.ceil((NOW - from) / bucket);
      const points =
        mockHistory(request(from, max), specs, gaps, NOW, 1000).series[0]
          ?.points ?? [];
      let sum = 0;
      for (const p of points) {
        const { min, max: hi, avg } = p;
        if (min === null || hi === null || avg === null) {
          throw new Error("a drawn point without its envelope");
        }
        expect(min).toBeLessThanOrEqual(avg);
        expect(hi).toBeGreaterThanOrEqual(avg);
        sum += hi - min;
      }
      return sum / points.length;
    };
    const hour = meanSpread(NOW - HOUR, 10_000);
    const month = meanSpread(
      Math.floor((NOW - 30 * DAY) / (30 * MIN)) * 30 * MIN,
      30 * MIN
    );
    expect(month).toBeGreaterThan(2 * hour);
  });

  it("answers 7d stepped back past the roll cut from quarters", () => {
    const older = mockHistory(
      { ...request(NOW - 14 * DAY, 1008), to_ms: NOW - 7 * DAY },
      specs,
      gaps,
      NOW,
      1000
    );
    expect(older.tier).toBe("m15");
    expect(older.bucket_ms).toBe(15 * MIN);
    // 1,008 slots asked; 672 quarters fit, so none are merged.
    const ts = (older.series[0]?.points ?? []).map((p) => p.t);
    expect(ts.every((t) => t % (15 * MIN) === 0)).toBe(true);
  });
});

describe("mockHistory through now (D-092)", () => {
  const hourAt = (metric: string, labels: [string, string][] = []) =>
    mockHistory(
      {
        ...request(NOW - HOUR, 400),
        selectors: [{ metric, labels }],
      },
      specs,
      gaps,
      NOW,
      1000
    );

  it("holds each series for its slowest period, or the bucket", () => {
    // 1 s CPU: the 10 s bucket. 60 s disk usage: 2.5 periods.
    expect(hourAt("cpu.total").series[0]?.hold_ms).toBe(10_000);
    const disk = hourAt("disk.used", [["vol", "/"]]).series[0];
    expect(disk?.hold_ms).toBe(150_000);
    // An adaptive collector slows to 10 s with only the tray open.
    expect(hourAt("gpu.freq").series[0]?.hold_ms).toBe(25_000);
  });

  it("answers through the open bucket", () => {
    const points = hourAt("cpu.total").series[0]?.points ?? [];
    expect(points.at(-1)?.t).toBe(Math.floor(NOW / 10_000) * 10_000);
  });
});

describe("mockRecentHistory (history unavailable)", () => {
  it("reads a fixed tier as asked, like history_recent", () => {
    // A two-hour minute read: Auto would pick 10 s buckets for it.
    const page = mockRecentHistory(
      { ...request(NOW - 2 * HOUR, 1000), tier: "m1" },
      specs,
      NOW,
      1000
    );
    expect(page.tier).toBe("m1");
    expect(page.bucket_ms).toBe(MIN);
  });

  it("picks the tier for Auto, and has values for the last hour only", () => {
    const page = mockRecentHistory(
      request(NOW - 2 * HOUR, 1000),
      specs,
      NOW,
      1000
    );
    expect(page.tier).toBe("s10");
    expect(page.gaps).toEqual([]);
    const points = page.series[0]?.points ?? [];
    expect(points.length).toBeGreaterThan(0);
    for (const p of points)
      expect(p.t + page.bucket_ms).toBeGreaterThan(NOW - HOUR);
  });
});
