import type { Gap } from "@core/generated/bindings";
import {
  layoutMarkers,
  type Marker,
  sleepMarkers,
  timelineBands,
} from "./gaps";
import { axisTicks } from "./time";

const MIN = 60_000;
const at = (h: number, m = 0) => new Date(2026, 9, 4, h, m).getTime();
const sleep = (start: number, end: number | null): Gap => ({
  start_ms: start,
  end_ms: end,
  module: null,
  reason: "sleep",
});

describe("timelineBands", () => {
  it("says how long on 24h and from when to when on 1h", () => {
    const g = sleep(at(1, 40), at(6, 55));
    expect(timelineBands([g], at(0), at(23), "24h")[0]?.label).toBe(
      "Asleep 5h 15m · no samples"
    );
    expect(timelineBands([g], at(0), at(23), "1h")[0]?.label).toBe(
      "Asleep 01:40–06:55 · not interpolated"
    );
  });

  it("keeps one band for the same gap returned by every lane's page", () => {
    const gaps = [
      sleep(at(1, 40), at(6, 55)),
      sleep(at(1, 40) + 3, at(6, 55) + 3),
      sleep(at(1, 40), at(6, 55)),
    ];
    expect(timelineBands(gaps, at(0), at(23), "24h")).toHaveLength(1);
  });

  it("keeps every module's gaps, each scoped to its module", () => {
    const g = (module: "gpu" | "cpu"): Gap => ({
      start_ms: at(3),
      end_ms: at(4),
      module,
      reason: "module_disabled",
    });
    const bands = timelineBands([g("gpu"), g("cpu")], at(0), at(23), "24h");
    expect(bands.map((b) => b.module).sort()).toEqual(["cpu", "gpu"]);
  });
});

describe("sleepMarkers", () => {
  it("puts Sleep at a sleep gap's start and Wake at its end", () => {
    const markers = sleepMarkers([sleep(at(1, 40), at(6, 55))], at(0), at(23));
    expect(markers.map((m) => m.label)).toEqual(["01:40 Sleep", "06:55 Wake"]);
  });

  it("has no Wake for a gap that is still open", () => {
    const markers = sleepMarkers([sleep(at(22), null)], at(0), at(23));
    expect(markers.map((m) => m.kind)).toEqual(["sleep"]);
  });
});

describe("layoutMarkers", () => {
  const measure = () => 100;
  const m = (tMs: number, label: string): Marker => ({
    tMs,
    kind: "sleep",
    label,
  });

  it("never overlaps pills on a row", () => {
    const placed = layoutMarkers(
      [m(0, "a"), m(10 * MIN, "b"), m(11 * MIN, "c")],
      0,
      100 * MIN,
      1000,
      measure
    );
    for (const row of [0, 1]) {
      const onRow = placed.filter((p) => p.row === row);
      for (let i = 1; i < onRow.length; i++) {
        const prev = onRow[i - 1] as (typeof onRow)[number];
        expect(
          (onRow[i] as (typeof onRow)[number]).leftPx
        ).toBeGreaterThanOrEqual(prev.leftPx + 100);
      }
    }
    expect(placed.map((p) => p.row)).toEqual([0, 1, 0].slice(0, placed.length));
  });

  it("moves a colliding pill to the second row, then merges into +N", () => {
    const placed = layoutMarkers(
      [m(0, "a"), m(MIN, "b"), m(2 * MIN, "c"), m(3 * MIN, "d")],
      0,
      100 * MIN,
      1000,
      measure
    );
    expect(placed.map((p) => [p.label, p.row, p.more])).toEqual([
      ["a", 0, 0],
      ["b", 1, 2],
    ]);
  });

  it("ends an event pill at its time, pulled inside at the left edge", () => {
    const ev = (tMs: number, label: string): Marker => ({
      tMs,
      kind: "event",
      label,
    });
    const placed = layoutMarkers(
      [ev(5 * MIN, "early"), ev(50 * MIN, "e1"), ev(55 * MIN, "e2")],
      0,
      100 * MIN,
      1000,
      measure
    );
    expect(placed.map((p) => [p.label, p.leftPx, p.row])).toEqual([
      ["early", 0, 0],
      // The dot sits on the time: the pill ends 4 px past x = 500.
      ["e1", 404, 0],
      ["e2", 454, 1],
    ]);
  });

  it("merges a crowded marker into the nearest pill, not the last placed", () => {
    const placed = layoutMarkers(
      [m(0, "a"), m(MIN, "b"), m(50 * MIN, "far"), m(2 * MIN, "c")],
      0,
      100 * MIN,
      1000,
      measure
    );
    expect(placed.map((p) => [p.label, p.more])).toEqual([
      ["a", 0],
      ["b", 1],
      ["far", 0],
    ]);
  });

  it("pulls a pill at the right edge back inside", () => {
    const [p] = layoutMarkers(
      [m(100 * MIN, "end")],
      0,
      100 * MIN,
      1000,
      measure
    );
    expect(p?.leftPx).toBe(900);
  });
});

describe("axisTicks", () => {
  it("ticks every 3 hours on the local hour for 24h", () => {
    const ticks = axisTicks(at(0, 30), at(23, 59), "24h");
    expect(ticks.map((t) => t.label)).toEqual([
      "03:00",
      "06:00",
      "09:00",
      "12:00",
      "15:00",
      "18:00",
      "21:00",
    ]);
    // A tick in the last 10% would collide with the "now" label.
    expect(axisTicks(at(0, 30), at(21, 40), "24h").at(-1)?.label).toBe("18:00");
  });

  it("ticks every 10 minutes for 1h and leaves room for the now label", () => {
    const ticks = axisTicks(at(9, 3), at(10, 3), "1h");
    expect(ticks.map((t) => t.label)).toEqual([
      "09:10",
      "09:20",
      "09:30",
      "09:40",
      "09:50",
    ]);
  });
});
