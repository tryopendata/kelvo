/**
 * The two-phase live history (D-066) and the clock-step restart (D-064):
 * `backfill_earlier` chunks prepended in front of the ring, and a new
 * `layout_no` with an older frame restarting it.
 */
import type { LiveMsg, SeriesKey } from "@core/generated/bindings";
import {
  type HostLive,
  initialHostLive,
  RowRing,
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

const layout = (no = 1): LiveMsg => ({
  kind: "layout",
  kinds: [],
  layout_no: no,
  series: SERIES,
});
const status: LiveMsg = {
  kind: "status",
  interval_ms: 1000,
  frame_period_ms: 1000,
  paused: false,
  display_idle: false,
  on_battery: false,
  performance: "off",
  power_source: "adapter",
  primary_iface: null,
};

/** `n` rows from `startMs`, one per second; cpu.total is the second. */
const rowsMsg = (
  kind: "backfill" | "backfill_earlier",
  startMs: number,
  n: number,
  layoutNo = 1,
  timeline = 0
): LiveMsg => {
  const body = {
    layout_no: layoutNo,
    start_ms: startMs,
    interval_ms: 1000,
    rows: Array.from({ length: n }, (_, i) => [startMs / 1000 + i, null, 1]),
    holds_ms: [],
  };
  return kind === "backfill" ? { kind, timeline, ...body } : { kind, ...body };
};

const frame = (tsMs: number, layoutNo = 1, timeline = 0): LiveMsg => ({
  kind: "frame",
  ts_ms: tsMs,
  layout_no: layoutNo,
  timeline,
  values: [tsMs / 1000, 1, 2],
  held: [tsMs / 1000, 1, 2],
});

function columnTimes(s: HostLive): number[] {
  const out: number[] = [];
  for (let i = 0; i < s.columns.length; i++) out.push(s.columns.tsAt(i));
  return out;
}

const rowTimes = (s: HostLive) => s.rows.since(0).map((r) => r.tsMs);
const secs = (...t: number[]) => t.map((v) => v * 1000);

describe("backfill_earlier", () => {
  it("prepends chunks newest first, keeping rows and columns in time order", () => {
    const s = apply([
      status,
      layout(),
      rowsMsg("backfill", 10_000, 3),
      frame(13_000),
      rowsMsg("backfill_earlier", 7000, 3),
      rowsMsg("backfill_earlier", 4000, 3),
    ]);
    const expected = secs(4, 5, 6, 7, 8, 9, 10, 11, 12, 13);
    expect(columnTimes(s)).toEqual(expected);
    expect(rowTimes(s)).toEqual(expected);
    // Each slot holds its own row's value (cpu.total is the second).
    const col = s.columns.column("cpu.total");
    expect(col?.[s.columns.slot(0)]).toBe(4);
    expect(col?.[s.columns.slot(3)]).toBe(7);
    expect(s.lastTsMs).toBe(13_000);
    expect(seriesWindow(s, "cpu.total", 10_000).values).toEqual([
      4, 5, 6, 7, 8, 9, 10, 11, 12, 13,
    ]);
  });

  it("drops chunk rows that overlap what is held", () => {
    const s = apply([
      status,
      layout(),
      rowsMsg("backfill", 10_000, 3),
      // 8 to 11 s: 10 and 11 are already held.
      rowsMsg("backfill_earlier", 8000, 4),
    ]);
    expect(columnTimes(s)).toEqual(secs(8, 9, 10, 11, 12));
    expect(rowTimes(s)).toEqual(secs(8, 9, 10, 11, 12));
  });

  it("keeps the newest history when the ring is full", () => {
    const small: HostLive = {
      ...initialHostLive("h1"),
      rows: new RowRing(4),
      columns: new SeriesColumns(4),
    };
    const s = apply(
      [
        status,
        layout(),
        rowsMsg("backfill", 10_000, 2),
        rowsMsg("backfill_earlier", 5000, 5),
      ],
      small
    );
    expect(columnTimes(s)).toEqual(secs(8, 9, 10, 11));
    expect(rowTimes(s)).toEqual(secs(8, 9, 10, 11));
  });

  it("bumps rowsEpoch for prepended rows, not for appended ones", () => {
    const a = apply([status, layout(), rowsMsg("backfill", 10_000, 3)]);
    const b = apply([frame(13_000)], a);
    expect(b.rowsEpoch).toBe(a.rowsEpoch);
    const c = apply([rowsMsg("backfill_earlier", 7000, 3)], b);
    expect(c.rowsEpoch).toBe(b.rowsEpoch + 1);
    expect(c.rowsVersion).toBe(b.rowsVersion + 1);
    // A chunk that is all overlap changes nothing.
    const d = apply([rowsMsg("backfill_earlier", 10_000, 2)], c);
    expect(d).toBe(c);
  });
});

describe("stored history in the ring (LiveHub::warm)", () => {
  // The rows a restarted engine reads back from the 10 s tier: their own
  // layout number, 10 s apart, each series held 25 s.
  const WARM = 4_294_967_295;
  const warmLayout: LiveMsg = {
    kind: "layout",
    kinds: ["mean", "mean", "mean"],
    layout_no: WARM,
    series: SERIES,
  };
  const stored = (startMs: number, n: number): LiveMsg => ({
    kind: "backfill_earlier",
    layout_no: WARM,
    start_ms: startMs,
    interval_ms: 10_000,
    rows: Array.from({ length: n }, (_, i) => [i + 1, null, 1]),
    holds_ms: [25_000, 25_000, 25_000],
  });

  it("fills the chart grid before the live rows, without holes", () => {
    const s = apply([
      status,
      warmLayout,
      layout(),
      rowsMsg("backfill", 100_000, 3),
      frame(103_000),
      // Buckets ending at 40, 50 and 60 s; the run restarted at 100 s.
      stored(40_000, 3),
    ]);
    const w = seriesWindow(s, "cpu.total", 80_000).values;
    // Slot i is 24 s + i seconds. A mean covers the span before its sample.
    const at = (sec: number) => w[sec - 24];
    // The oldest sample has none before it, so it covers only its own slot.
    expect(at(39)).toBeNull();
    expect(at(40)).toBe(1);
    expect(at(41)).toBe(2);
    expect(at(50)).toBe(2);
    expect(at(55)).toBe(3);
    expect(at(60)).toBe(3);
    // Kelvo was not running between 60 and 100 s: a hole, not a line.
    expect(at(70)).toBeNull();
    expect(at(99)).toBeNull();
    expect(at(100)).toBe(100);
  });
});

/** Frames every second from `fromS` to `toS` seconds, timeline 0. */
const frames = (fromS: number, toS: number): LiveMsg[] =>
  Array.from({ length: toS - fromS + 1 }, (_, i) => frame((fromS + i) * 1000));

describe("clock steps (timeline)", () => {
  it("drops only the rows at or after a new timeline's first row", () => {
    let s = apply([status, layout(1), ...frames(30, 101)]);
    const epoch = s.rowsEpoch;
    s = apply([frame(40_000, 1, 1)], s);
    expect(rowTimes(s)).toEqual(
      secs(30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40)
    );
    expect(columnTimes(s)).toEqual(rowTimes(s));
    expect(s.lastTsMs).toBe(40_000);
    expect(s.held["cpu.total"]).toBe(40);
    expect(s.rowsEpoch).toBe(epoch + 1);
    // The new timeline keeps going.
    s = apply([frame(41_000, 1, 1)], s);
    expect(seriesWindow(s, "cpu.total", 3000).values).toEqual([39, 40, 41]);
  });

  it("keeps the hour when a small step back overlaps a second of it", () => {
    let s = apply([status, layout(1), ...frames(1, 3600)]);
    s = apply([frame(3_599_400, 1, 1)], s);
    // 3,600 s went; 1 to 3,599 s stay, then the stepped row.
    expect(rowTimes(s).slice(-3)).toEqual([3_598_000, 3_599_000, 3_599_400]);
    expect(s.columns.length).toBe(3600);
    expect(s.columns.firstTsMs()).toBe(1000);
    expect(s.lastTsMs).toBe(3_599_400);
  });

  it("drops an older frame on the same timeline as a duplicate", () => {
    let s = apply([status, layout(1), frame(100_000)]);
    s = apply([frame(40_000)], s);
    expect(rowTimes(s)).toEqual([100_000]);
    expect(s.lastTsMs).toBe(100_000);
  });

  /**
   * #12: after a module toggle (new layout) a reconnect asks for two
   * intervals of overlap. A new layout with an older time used to read as a
   * clock step and clear the ring; on the same timeline it is a duplicate.
   */
  it("does not reset on a new layout with an older time", () => {
    let s = apply([status, layout(1), ...frames(90, 100)]);
    const epoch = s.rowsEpoch;
    s = apply([layout(2), rowsMsg("backfill", 99_000, 3, 2)], s);
    expect(rowTimes(s)).toEqual(
      secs(90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101)
    );
    expect(s.rowsEpoch).toBe(epoch);
  });

  it("truncates at a resume backfill on a new timeline", () => {
    let s = apply([status, layout(1), ...frames(30, 100)]);
    s = apply([rowsMsg("backfill", 35_000, 2, 1, 1)], s);
    expect(rowTimes(s)).toEqual(secs(30, 31, 32, 33, 34, 35, 36));
    expect(columnTimes(s)).toEqual(rowTimes(s));
    expect(s.lastTsMs).toBe(36_000);
  });
});
