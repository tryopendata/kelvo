import {
  type LiveMsg,
  RING_SPAN_MS,
  type SeriesKey,
} from "@core/generated/bindings";
import {
  type HostLive,
  initialHostLive,
  reduceLive,
  seriesWindow,
} from "./live-state";
import { lastMeasuredMs, readFailure, readFailureMs } from "./read-failure";

const SERIES: SeriesKey[] = [
  { metric: "cpu.total", labels: [] },
  { metric: "thermal.hottest", labels: [] },
];

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

const frame = (
  ts: number,
  values: (number | null)[],
  held = values,
  layout_no = 1
): LiveMsg => ({
  kind: "frame",
  ts_ms: ts,
  layout_no,
  timeline: 0,
  values,
  held,
});

function apply(msgs: LiveMsg[], state: HostLive = initialHostLive("h1")) {
  return msgs.reduce(reduceLive, state);
}

const base = [
  status(),
  { kind: "layout", layout_no: 1, series: SERIES, kinds: [] },
] as [LiveMsg, LiveMsg];

describe("readFailure", () => {
  it("is fine while the series has a current value", () => {
    const s = apply([...base, frame(1000, [10, 60])]);
    expect(readFailure(s, "thermal.hottest")).toBeNull();
    expect(readFailureMs(s, "thermal.hottest")).toBeUndefined();
  });

  it("reports a listed series that lost its held value, with the last good time", () => {
    const s = apply([
      ...base,
      frame(1000, [10, 60]),
      frame(2000, [11, null], [11, 60]),
      frame(3000, [12, null], [12, null]),
    ]);
    expect(readFailure(s, "thermal.hottest")).toEqual({ lastGoodMs: 1000 });
    expect(readFailure(s, "cpu.total")).toBeNull();
  });

  it("has no last good time for a series never measured", () => {
    const s = apply([
      ...base,
      frame(1000, [10, null], [10, null]),
      frame(2000, [11, null], [11, null]),
    ]);
    expect(readFailure(s, "thermal.hottest")).toEqual({ lastGoodMs: null });
    expect(lastMeasuredMs(initialHostLive("h1"), "thermal.hottest")).toBeNull();
  });

  it("looks back one ring span (an hour) from the newest row, no further", () => {
    // Measured at 1 s and 2 s, then failing every second for an hour.
    let s = apply([...base, frame(1000, [10, 60]), frame(2000, [11, 61])]);
    const failing = (ts: number) => frame(ts, [1, null], [1, null]);
    const end = 2000 + RING_SPAN_MS;
    for (let ts = 3000; ts < end; ts += 1000) s = reduceLive(s, failing(ts));
    // 2 s is just inside the span ending at `end - 1000`.
    expect(readFailureMs(s, "thermal.hottest")).toBe(2000);
    // At `end` it sits exactly one span back: outside, like 1 s.
    s = reduceLive(s, failing(end));
    expect(readFailureMs(s, "thermal.hottest")).toBeNull();
  });

  it("finds a sample taken under an earlier layout with another index", () => {
    const s = apply([
      ...base,
      frame(1000, [10, 60]),
      {
        kind: "layout",
        layout_no: 2,
        series: [SERIES[1] as SeriesKey, SERIES[0] as SeriesKey],
        kinds: [],
      },
      frame(2000, [null, 11], [null, 11], 2),
    ]);
    expect(readFailureMs(s, "thermal.hottest")).toBe(1000);
  });

  it("does not report while paused, stale or for a series not in the layout", () => {
    const failed = apply([...base, frame(1000, [10, null], [10, null])]);
    expect(readFailure(failed, "thermal.hottest")).not.toBeNull();
    expect(
      readFailure(reduceLive(failed, status(true)), "thermal.hottest")
    ).toBeNull();
    expect(
      readFailure({ ...failed, stale: true }, "thermal.hottest")
    ).toBeNull();
    expect(readFailure(failed, "fan.rpm{fan=0}")).toBeNull();
  });
});

describe("series disappeared", () => {
  it("draws the slots after the series left the layout as a gap, not the last value", () => {
    const s = apply([
      ...base,
      frame(1000, [10, 60]),
      frame(2000, [11, 61]),
      {
        kind: "layout",
        layout_no: 2,
        series: [SERIES[0] as SeriesKey],
        kinds: [],
      },
      frame(3000, [12], [12], 2),
      frame(4000, [13], [13], 2),
    ]);
    const w = seriesWindow(s, "thermal.hottest", 4000);
    expect(w.values).toEqual([60, 61, null, null]);
    // Gone from the layout is "not measured", not a failed read.
    expect(readFailure(s, "thermal.hottest")).toBeNull();
  });
});
