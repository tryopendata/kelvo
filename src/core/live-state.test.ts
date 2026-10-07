import type { LiveMsg, MetricKind, SeriesKey } from "@core/generated/bindings";
import {
  type HostLive,
  initialHostLive,
  reduceLive,
  SeriesColumns,
  seriesWindow,
} from "./live-state";

const SERIES: SeriesKey[] = [
  { metric: "cpu.total", labels: [] },
  { metric: "cpu.load", labels: [["core", "P0"]] },
  { metric: "cpu.load", labels: [["core", "E0"]] },
];

function apply(msgs: LiveMsg[], state: HostLive = initialHostLive("h1")) {
  return msgs.reduce(reduceLive, state);
}

const layout: LiveMsg = {
  kind: "layout",
  layout_no: 1,
  series: SERIES,
  kinds: [],
};
const status = (paused = false): LiveMsg => ({
  kind: "status",
  interval_ms: 1000,
  frame_period_ms: 1000,
  paused,
  display_idle: false,
  on_battery: false,
  performance: "off",
  power_source: "adapter",
  primary_iface: null,
});

describe("reduceLive", () => {
  it("indexes the layout by display key and metric", () => {
    const s = apply([layout]);
    expect(s.layoutNo).toBe(1);
    expect(s.layouts[1]?.keys).toEqual([
      "cpu.total",
      "cpu.load{core=P0}",
      "cpu.load{core=E0}",
    ]);
    expect(s.layouts[1]?.byMetric.get("cpu.load")).toEqual([
      "cpu.load{core=P0}",
      "cpu.load{core=E0}",
    ]);
  });

  it("seeds readouts from the backfill's last row and goes live", () => {
    const s = apply([
      status(),
      layout,
      {
        kind: "backfill",
        holds_ms: [],
        layout_no: 1,
        timeline: 0,
        start_ms: 1000,
        interval_ms: 1000,
        rows: [
          [10, 1, 2],
          [20, 3, null],
        ],
      },
    ]);
    expect(s.connection).toBe("live");
    expect(s.held).toEqual({
      "cpu.total": 20,
      "cpu.load{core=P0}": 3,
      "cpu.load{core=E0}": null,
    });
    expect(s.rows.length).toBe(2);
    expect(s.lastTsMs).toBe(2000);
  });

  it("replaces held from a frame's held array, not its raw values", () => {
    const s = apply([
      status(),
      layout,
      {
        kind: "frame",
        ts_ms: 5000,
        layout_no: 1,
        timeline: 0,
        values: [null, 4, 5],
        held: [42, 4, 5],
      },
    ]);
    expect(s.held["cpu.total"]).toBe(42);
    expect(s.rows.last()?.values).toEqual([null, 4, 5]);
  });

  it("drops a frame or backfill row that is not newer than the ring", () => {
    const frame = (ts: number): LiveMsg => ({
      kind: "frame",
      ts_ms: ts,
      layout_no: 1,
      timeline: 0,
      values: [ts, 0, 0],
      held: [ts, 0, 0],
    });
    const s = apply([layout, frame(3000), frame(2000), frame(3000)]);
    expect(s.rows.length).toBe(1);
  });

  it("ignores frames for an unknown layout", () => {
    const before = apply([status()]);
    const after = reduceLive(before, {
      kind: "frame",
      ts_ms: 1,
      layout_no: 9,
      timeline: 0,
      values: [1],
      held: [1],
    });
    expect(after).toBe(before);
  });

  it("clears stale on a frame and on pause", () => {
    const stale = { ...apply([status(), layout]), stale: true };
    expect(reduceLive(stale, status(true)).stale).toBe(false);
    expect(
      reduceLive(stale, {
        kind: "frame",
        ts_ms: 1,
        layout_no: 1,
        timeline: 0,
        values: [1, 1, 1],
        held: [1, 1, 1],
      }).stale
    ).toBe(false);
  });

  describe("statusSinceMs: when the current sampling setup's frames began", () => {
    const frame = (ts: number, timeline = 0): LiveMsg => ({
      kind: "frame",
      ts_ms: ts,
      layout_no: 1,
      timeline,
      values: [1, 1, 1],
      held: [1, 1, 1],
    });
    const backfill = (start: number): LiveMsg => ({
      kind: "backfill",
      holds_ms: [],
      layout_no: 1,
      timeline: 0,
      start_ms: start,
      interval_ms: 1000,
      rows: [[1, 1, 1]],
    });

    it("starts at the first frame and holds while the setup is unchanged", () => {
      const s = apply([status(), layout, frame(1000), frame(2000), status()]);
      expect(s.statusSinceMs).toBe(1000);
    });

    it("restarts at the next frame after the interval changes", () => {
      const s = apply([status(), layout, frame(1000)]);
      const slower: LiveMsg = {
        ...(status() as Extract<LiveMsg, { kind: "status" }>),
        interval_ms: 30_000,
      };
      const changed = apply([slower], s);
      expect(changed.statusSinceMs).toBeNull();
      expect(apply([frame(31_000)], changed).statusSinceMs).toBe(31_000);
    });

    it("restarts after a backfill: those rows were taken with this window hidden", () => {
      const s = apply([
        status(),
        layout,
        frame(1000),
        backfill(2000),
        frame(10_000),
      ]);
      expect(s.statusSinceMs).toBe(10_000);
    });

    it("restarts after a clock step, so earlier times are not skipped", () => {
      const s = apply([status(), layout, frame(50_000), frame(51_000, 1)]);
      expect(s.statusSinceMs).toBe(51_000);
      const back = apply([frame(20_000, 2)], s);
      expect(back.statusSinceMs).toBe(20_000);
    });
  });

  it("stores capabilities and process rows", () => {
    const s = apply([
      {
        kind: "processes",
        ts_ms: 7,
        rows: [],
      },
    ]);
    expect(s.processes).toEqual({ tsMs: 7, rows: [] });
  });
});

describe("seriesWindow", () => {
  it("lays rows on the interval grid with null for missing slots", () => {
    const s = apply([
      status(),
      layout,
      {
        kind: "backfill",
        holds_ms: [],
        layout_no: 1,
        timeline: 0,
        start_ms: 1000,
        interval_ms: 1000,
        rows: [
          [1, 0, 0],
          [2, 0, 0],
        ],
      },
      // A 3 s hole (sleep), then two more rows.
      {
        kind: "backfill",
        holds_ms: [],
        layout_no: 1,
        timeline: 0,
        start_ms: 6000,
        interval_ms: 1000,
        rows: [
          [6, 0, 0],
          [null, 0, 0],
        ],
      },
    ]);
    const w = seriesWindow(s, "cpu.total", 8000);
    expect(w.tEndMs).toBe(7000);
    // Slots end at 7000: 0..7000 → [0,1,2,3,4,5,6,7]s.
    expect(w.values).toEqual([null, 1, 2, null, null, null, 6, null]);
  });

  it("reads 2 s frames at a 1 s interval as one run (Performance mode)", () => {
    const frame = (ts: number): LiveMsg => ({
      kind: "frame",
      ts_ms: ts,
      layout_no: 1,
      timeline: 0,
      values: [ts / 1000, 0, 0],
      held: [ts / 1000, 0, 0],
    });
    const s = apply([
      { ...status(), frame_period_ms: 2000, performance: "setting" } as LiveMsg,
      layout,
      ...[2000, 4000, 6000, 8000, 10_000].map(frame),
    ]);
    const w = seriesWindow(s, "cpu.total", 10_000);
    expect(w.intervalMs).toBe(2000);
    expect(w.values).toEqual([2, 4, 6, 8, 10]);
  });

  describe("joins samples by the holds the host published (D-090)", () => {
    const layoutOf = (kind: MetricKind): LiveMsg => ({
      kind: "layout",
      layout_no: 1,
      series: SERIES,
      kinds: [kind, kind, kind],
    });
    const holds = (ms: number): LiveMsg => ({
      kind: "holds",
      layout_no: 1,
      holds_ms: [ms, ms, ms],
    });
    const frame = (ts: number): LiveMsg => ({
      kind: "frame",
      ts_ms: ts,
      layout_no: 1,
      timeline: 0,
      values: [ts / 1000, 0, 0],
      held: [ts / 1000, 0, 0],
    });
    const at = (interval_ms: number): LiveMsg => ({
      ...(status() as Extract<LiveMsg, { kind: "status" }>),
      interval_ms,
      frame_period_ms: interval_ms,
    });
    // 2 s rows on battery (held 5 s), then 1 s rows after plugging in.
    const pluggedIn = (kind: MetricKind) =>
      apply([
        at(2000),
        layoutOf(kind),
        holds(5000),
        ...[2000, 4000, 6000, 8000].map(frame),
        at(1000),
        holds(2500),
        ...[9000, 10_000].map(frame),
      ]);

    it("draws a gauge's coarser samples on a finer grid as one line", () => {
      const w = seriesWindow(pluggedIn("gauge"), "cpu.total", 9000);
      expect(w.intervalMs).toBe(1000);
      // The slots between two samples are the line the chart draws anyway.
      expect(w.values).toEqual([2, 3, 4, 5, 6, 7, 8, 9, 10]);
    });

    it("fills a mean's slots with the sample that averages them", () => {
      // Each 2 s sample is the mean over the two seconds before it.
      expect(seriesWindow(pluggedIn("mean"), "cpu.total", 9000).values).toEqual(
        [2, 4, 4, 6, 6, 8, 8, 9, 10]
      );
    });

    it("draws 1 s history on a 2 s grid without holes after unplugging", () => {
      const s = apply([
        at(1000),
        layoutOf("gauge"),
        holds(2500),
        ...[1000, 2000, 3000, 4000, 5000].map(frame),
        at(2000),
        holds(5000),
        ...[7000, 9000].map(frame),
      ]);
      const w = seriesWindow(s, "cpu.total", 8000);
      expect(w.intervalMs).toBe(2000);
      expect(w.values).toEqual([4, 5, 7, 9]);
    });

    it("breaks where samples are further apart than their hold", () => {
      const s = apply([
        at(2000),
        layoutOf("gauge"),
        holds(5000),
        // 2 s rows, then a 6 s hole (sleep), then 1 s rows.
        ...[2000, 4000, 10_000].map(frame),
        at(1000),
        holds(2500),
        frame(11_000),
      ]);
      expect(seriesWindow(s, "cpu.total", 10_000).values).toEqual([
        2,
        3,
        4,
        null,
        null,
        null,
        null,
        null,
        10,
        11,
      ]);
    });

    it("covers a 10 s tray-only series without breaking the run", () => {
      // power.gpu with no window open: one sample in ten 1 s rows, held 25 s.
      const s = apply([
        status(),
        layoutOf("mean"),
        {
          kind: "backfill",
          layout_no: 1,
          timeline: 0,
          start_ms: 1000,
          interval_ms: 1000,
          rows: Array.from({ length: 21 }, (_, i) =>
            i % 10 === 0 ? [i, 0, 0] : [null, 0, 0]
          ),
          holds_ms: [25_000, 25_000, 25_000],
        },
      ]);
      const w = seriesWindow(s, "cpu.total", 20_000).values;
      expect(w).toEqual([...new Array(10).fill(10), ...new Array(10).fill(20)]);
    });

    it("lets a sample before the window cover the slots it averages", () => {
      const s = apply([
        status(),
        layoutOf("mean"),
        {
          kind: "backfill",
          layout_no: 1,
          timeline: 0,
          start_ms: 1000,
          interval_ms: 1000,
          rows: [
            [1, 0, 0],
            [null, 0, 0],
            [null, 0, 0],
            [4, 0, 0],
          ],
          holds_ms: [25_000, 25_000, 25_000],
        },
      ]);
      expect(seriesWindow(s, "cpu.total", 2000).values).toEqual([4, 4]);
    });

    it("joins nothing for a row with no published hold", () => {
      const s = apply([status(), layoutOf("mean"), ...[1000, 3000].map(frame)]);
      expect(seriesWindow(s, "cpu.total", 3000).values).toEqual([1, null, 3]);
    });
  });

  it("returns an all-null window before any data", () => {
    const w = seriesWindow(initialHostLive("h1"), "cpu.total", 3000);
    expect(w.values).toEqual([null, null, null]);
  });
});

/** Logical rows of one column, oldest first, `null` for NaN. */
function read(cols: SeriesColumns, key: string): (number | null)[] {
  const col = cols.column(key);
  return Array.from({ length: cols.length }, (_, i) => {
    const v = col?.[cols.slot(i)];
    return v === undefined || Number.isNaN(v) ? null : v;
  });
}

describe("SeriesColumns", () => {
  it("stores null and absent series as NaN, one column per key", () => {
    const c = new SeriesColumns(8);
    c.push(1000, ["a", "b"], [1, null]);
    // A new layout: b gone, c new.
    c.push(2000, ["a", "c"], [2, 5]);
    expect(read(c, "a")).toEqual([1, 2]);
    expect(read(c, "b")).toEqual([null, null]);
    expect(read(c, "c")).toEqual([null, 5]);
    expect(c.column("never")).toBeUndefined();
  });

  it("drops a row that is not newer than the newest", () => {
    const c = new SeriesColumns(8);
    c.push(2000, ["a"], [2]);
    c.push(2000, ["a"], [9]);
    c.push(1000, ["a"], [9]);
    expect(read(c, "a")).toEqual([2]);
  });

  it("wraps at capacity, keeping the newest rows in order", () => {
    const c = new SeriesColumns(4);
    for (let i = 1; i <= 6; i++) c.push(i * 1000, ["a"], [i]);
    expect(c.length).toBe(4);
    expect(read(c, "a")).toEqual([3, 4, 5, 6]);
    expect(c.tsAt(0)).toBe(3000);
    expect(c.lastTsMs()).toBe(6000);
    expect(c.firstAfter(4500)).toBe(2);
    expect(c.firstAfter(0)).toBe(0);
    expect(c.firstAfter(6000)).toBe(4);
  });
});

describe("seriesWindow over a wrapped ring", () => {
  it("reads the window across the wrap point with gaps as null", () => {
    let s = apply([status(), layout]);
    s = { ...s, columns: new SeriesColumns(5) };
    for (let t = 1; t <= 7; t++) {
      s = reduceLive(s, {
        kind: "frame",
        ts_ms: t * 1000,
        layout_no: 1,
        timeline: 0,
        values: [t === 6 ? null : t, 0, 0],
        held: [t, 0, 0],
      });
    }
    // Ring holds 3..7 s; a 6 s window ends at 7 s: slots 2..7 s.
    expect(seriesWindow(s, "cpu.total", 6000).values).toEqual([
      null,
      3,
      4,
      5,
      null,
      7,
    ]);
  });
});
