import {
  axisTicks,
  clock,
  clockSeconds,
  dayClock,
  dayLabel,
  endLabel,
  heatmapCellView,
  historyWindow,
  liveView,
  momentLabel,
  monthDay,
  rangeSubtitle,
  requestBucketMs,
  resolutionLabel,
  SPAN_MS,
  stepRange,
  stepView,
} from "./time";

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;
/** Local time, so the labels read the same in any time zone. */
const at = (month: number, day: number, h = 0, m = 0) =>
  new Date(2026, month - 1, day, h, m).getTime();

describe("requestBucketMs", () => {
  it("keeps 1h and 24h on their tier's own buckets", () => {
    expect(requestBucketMs("1h", 820)).toBe(10_000);
    expect(requestBucketMs("24h", 820)).toBe(MIN);
  });

  it("asks 7d and 30d for at most 2 points per plot pixel, on a round width", () => {
    // An 820 px plot (1280 px window): 1,640 points at most.
    expect(requestBucketMs("7d", 820)).toBe(10 * MIN);
    expect(requestBucketMs("30d", 820)).toBe(30 * MIN);
    // Plan 4.6: about 2,000 points for 7d, 2,880 for 30d on a wide plot.
    expect(requestBucketMs("7d", 1100)).toBe(5 * MIN);
    expect(requestBucketMs("30d", 1440)).toBe(15 * MIN);
    for (const [span, px] of [
      ["7d", 640],
      ["7d", 1700],
      ["30d", 500],
      ["30d", 2400],
    ] as const) {
      expect(SPAN_MS[span] / requestBucketMs(span, px)).toBeLessThanOrEqual(
        2 * px
      );
    }
  });

  it("reads an unmeasured plot as 1,000 px", () => {
    expect(requestBucketMs("30d", 0)).toBe(requestBucketMs("30d", 1000));
  });
});

describe("historyWindow", () => {
  const NOW = at(10, 5, 22, 43) + 17_000;

  it("starts 7d on a bucket boundary within one bucket of the range", () => {
    const w = historyWindow("7d", NOW, 10 * MIN);
    expect(w.fromMs % (10 * MIN)).toBe(0);
    expect(NOW - SPAN_MS["7d"] - w.fromMs).toBeGreaterThanOrEqual(0);
    expect(NOW - SPAN_MS["7d"] - w.fromMs).toBeLessThan(10 * MIN);
  });

  it("keeps a bucket of slack before 1h and 24h", () => {
    const w = historyWindow("24h", NOW, MIN);
    expect(NOW - DAY - w.fromMs).toBeGreaterThanOrEqual(MIN);
  });

  it("asks for one point per bucket, the open one included", () => {
    const w = historyWindow("30d", NOW, 30 * MIN);
    expect(w.maxPoints).toBe(Math.ceil((NOW - w.fromMs) / (30 * MIN)));
    expect(w.maxPoints * 30 * MIN).toBeGreaterThanOrEqual(NOW - w.fromMs);
  });
});

describe("resolutionLabel", () => {
  it("names the bucket width in the largest whole unit", () => {
    expect(resolutionLabel(10_000)).toBe("10 s avg");
    expect(resolutionLabel(MIN)).toBe("1 min avg");
    expect(resolutionLabel(10 * MIN)).toBe("10 min avg");
    expect(resolutionLabel(30 * MIN)).toBe("30 min avg");
    expect(resolutionLabel(HOUR)).toBe("1 h avg");
    expect(resolutionLabel(2 * HOUR)).toBe("2 h avg");
  });
});

describe("stepRange", () => {
  const NOW = at(10, 4, 22, 40);

  it("steps back by one range length from the window drawn", () => {
    expect(stepRange("7d", NOW, -1, NOW)).toBe(NOW - 7 * DAY);
    expect(stepRange("30d", NOW - 30 * DAY, -1, NOW)).toBe(NOW - 60 * DAY);
  });

  it("steps forward and returns to Live once it reaches now", () => {
    expect(stepRange("7d", NOW - 14 * DAY, 1, NOW)).toBe(NOW - 7 * DAY);
    expect(stepRange("7d", NOW - 7 * DAY, 1, NOW)).toBeNull();
    expect(stepRange("30d", NOW - 20 * DAY, 1, NOW)).toBeNull();
  });
});

describe("rangeSubtitle", () => {
  it("names the end while following Live", () => {
    const s = rangeSubtitle("24h", at(10, 3, 22, 40), at(10, 4, 22, 40), true);
    expect(s.lead + s.times).toBe("Last 24 hours, ending Sun Oct 4 · 22:40");
    const w = rangeSubtitle("30d", at(9, 4, 22, 40), at(10, 4, 22, 40), true);
    expect(w.lead + w.times).toBe("Last 30 days, ending Sun Oct 4 · 22:40");
  });

  it("gives both ends of a window stepped back to", () => {
    const s = rangeSubtitle("7d", at(9, 20, 22, 40), at(9, 27, 22, 40), false);
    expect(s.lead + s.times).toBe(
      "7 days, Sun Sep 20 · 22:40 – Sun Sep 27 · 22:40"
    );
  });

  it("drops the end's day inside one day", () => {
    const s = rangeSubtitle("1h", at(10, 4, 21, 40), at(10, 4, 22, 40), false);
    expect(s.lead + s.times).toBe("One hour, Sun Oct 4 · 21:40 – 22:40");
  });
});

describe("momentLabel and endLabel", () => {
  it("dates moments on the ranges that span days", () => {
    const t = at(10, 3, 14, 20);
    expect(momentLabel("24h", t)).toBe("14:20");
    expect(momentLabel("7d", t)).toBe("Sat 14:20");
    expect(momentLabel("30d", t)).toBe("Oct 3");
    expect(endLabel("7d", t, false)).toBe("Oct 3 · 14:20");
    expect(endLabel("1h", t, false)).toBe("14:20");
    expect(endLabel("30d", t, true)).toBe("now");
  });
});

describe("axisTicks over days", () => {
  it("puts a tick on each local midnight for 7d", () => {
    const ticks = axisTicks(at(9, 27, 22, 40), at(10, 4, 22, 40) - 0, "7d");
    expect(ticks.map((t) => t.label)).toEqual([
      "Mon 28",
      "Tue 29",
      "Wed 30",
      "Thu 1",
      "Fri 2",
      "Sat 3",
      "Sun 4",
    ]);
  });

  it("drops the midnight next to the right edge", () => {
    // Oct 5 00:00 is within the last tenth of a week ending at 06:00.
    const ticks = axisTicks(at(9, 28, 6), at(10, 5, 6), "7d");
    expect(ticks.at(-1)?.label).toBe("Sun 4");
  });

  it("stays on midnight across a daylight saving change", () => {
    // US clocks fall back on Nov 1 2026; any zone without DST passes too.
    const ticks = axisTicks(at(10, 29, 12), at(11, 5, 12), "7d");
    expect(ticks.map((t) => t.label)).toEqual([
      "Fri 30",
      "Sat 31",
      "Sun 1",
      "Mon 2",
      "Tue 3",
      "Wed 4",
    ]);
    for (const t of ticks) {
      const d = new Date(t.tMs);
      expect(d.getHours()).toBe(0);
      expect(d.getMinutes()).toBe(0);
    }
  });

  it("puts a tick on each Monday for 30d", () => {
    const ticks = axisTicks(at(9, 4, 22, 40), at(10, 4, 22, 40), "30d");
    expect(ticks.map((t) => t.label)).toEqual([
      "Sep 7",
      "Sep 14",
      "Sep 21",
      "Sep 28",
    ]);
    for (const t of ticks) expect(new Date(t.tMs).getDay()).toBe(1);
  });
});

describe("heatmapCellView", () => {
  const now = at(10, 4, 14, 20);

  it("opens an hour within the last 7 days at 1h, ending at the hour's end", () => {
    expect(heatmapCellView(at(10, 2, 9), at(10, 2, 10), now)).toEqual({
      span: "1h",
      endMs: at(10, 2, 10),
    });
  });

  it("follows Live when the hour reaches now", () => {
    expect(heatmapCellView(at(10, 4, 14), at(10, 4, 15), now)).toEqual({
      span: "1h",
      endMs: null,
    });
  });

  it("opens 6 hours centred on an hour older than 7 days, where only quarters are left", () => {
    expect(heatmapCellView(at(9, 20, 14), at(9, 20, 15), now)).toEqual({
      span: "6h",
      endMs: at(9, 20, 17, 30),
    });
  });
});

describe("heatmapCellView on a fall-back day", () => {
  it("opens 6 hours over the two-hour 01:00 cell rather than half of it", () => {
    const now = at(10, 4, 14, 20);
    // A recent 01:00 cell that ran two hours (the clock went back once).
    const start = at(10, 2, 1);
    const end = start + 2 * HOUR;
    const view = heatmapCellView(start, end, now);
    expect(view.span).toBe("6h");
    // Centred on the cell: both of its hours are inside the window.
    expect(view.endMs).toBe(start + HOUR + 3 * HOUR);
    expect((view.endMs as number) - 6 * HOUR).toBeLessThanOrEqual(start);
  });
});

describe("stepView and liveView", () => {
  const now = at(10, 4, 14, 20);

  it("steps a preset like stepRange, following Live at the same span", () => {
    expect(stepView("24h", at(10, 3, 15), 1, now)).toEqual({
      span: "24h",
      endMs: null,
    });
    expect(stepView("24h", at(10, 3, 14), -1, now)).toEqual({
      span: "24h",
      endMs: at(10, 2, 14),
    });
  });

  it("a 6h window stepped onto now follows Live at 24h, as the Live button does", () => {
    expect(stepView("6h", at(10, 4, 11), 1, now)).toEqual({
      span: "24h",
      endMs: null,
    });
    expect(stepView("6h", at(9, 20, 17, 30), 1, now)).toEqual({
      span: "6h",
      endMs: at(9, 20, 23, 30),
    });
    expect(liveView("6h")).toEqual({ span: "24h", endMs: null });
    expect(liveView("7d")).toEqual({ span: "7d", endMs: null });
  });
});

describe("the 6h span", () => {
  it("reads minutes, ticks every hour and dates its end", () => {
    expect(SPAN_MS["6h"]).toBe(6 * HOUR);
    expect(requestBucketMs("6h", 820)).toBe(MIN);
    const ticks = axisTicks(at(9, 20, 11, 30), at(9, 20, 17, 30), "6h");
    expect(ticks.map((t) => t.label)).toEqual([
      "12:00",
      "13:00",
      "14:00",
      "15:00",
      "16:00",
    ]);
    expect(endLabel("6h", at(9, 20, 17, 30), false)).toBe("Sep 20 · 17:30");
    expect(
      rangeSubtitle("6h", at(9, 20, 11, 30), at(9, 20, 17, 30), false)
    ).toEqual({ lead: "6 hours, ", times: "Sun Sep 20 · 11:30 – 17:30" });
  });
});

describe("clock and date labels", () => {
  it("pads the clock to two digits", () => {
    const t = at(1, 4, 9, 5) + 7_000;
    expect(clock(t)).toBe("09:05");
    expect(clockSeconds(t)).toBe("09:05:07");
    expect(clock(at(12, 31, 0, 0))).toBe("00:00");
    expect(clockSeconds(at(12, 31, 23, 59) + 59_000)).toBe("23:59:59");
  });

  it("names every month and weekday in English", () => {
    const months = Array.from({ length: 12 }, (_, i) => monthDay(at(i + 1, 1)));
    expect(months).toEqual([
      "Jan 1",
      "Feb 1",
      "Mar 1",
      "Apr 1",
      "May 1",
      "Jun 1",
      "Jul 1",
      "Aug 1",
      "Sep 1",
      "Oct 1",
      "Nov 1",
      "Dec 1",
    ]);
    // Sun Oct 4 2026 to Sat Oct 10.
    const days = Array.from({ length: 7 }, (_, i) => dayLabel(at(10, 4 + i)));
    expect(days).toEqual([
      "Sun Oct 4",
      "Mon Oct 5",
      "Tue Oct 6",
      "Wed Oct 7",
      "Thu Oct 8",
      "Fri Oct 9",
      "Sat Oct 10",
    ]);
    expect(dayClock(at(10, 4, 7, 3))).toBe("Sun Oct 4 · 07:03");
    expect(momentLabel("7d", at(10, 5, 7, 3))).toBe("Mon 07:03");
  });

  it("labels 1h and 24h ticks with the clock", () => {
    const hour = axisTicks(at(10, 4, 9, 0), at(10, 4, 10, 0), "1h");
    expect(hour.map((t) => t.label)).toEqual([
      "09:00",
      "09:10",
      "09:20",
      "09:30",
      "09:40",
      "09:50",
    ]);
    const day = axisTicks(at(10, 3, 22, 40), at(10, 4, 22, 40), "24h");
    expect(day.map((t) => t.label)).toEqual([
      "00:00",
      "03:00",
      "06:00",
      "09:00",
      "12:00",
      "15:00",
      "18:00",
    ]);
  });
});
