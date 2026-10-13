import type { DockerHistoryPoint } from "@core/generated/bindings";
import { mockContainer } from "@core/mock/docker";
import { describe, expect, it } from "vitest";
import {
  buildSeriesRuns,
  nextMembership,
  pointValue,
  rankCandidates,
} from "./series-lines";

const metrics = mockContainer(0, 0).metrics;
const point = (
  time: number,
  patch: Partial<DockerHistoryPoint> = {}
): DockerHistoryPoint => ({
  time_ms: time,
  duration_ms: 10_000,
  metrics: { ...metrics, cpu_pct: 10 },
  peak: { ...metrics, cpu_pct: 30 },
  minimum: { ...metrics, cpu_pct: 2 },
  cpu_measured_ms: 9999,
  memory_measured_ms: 9999,
  cpu_covered: true,
  memory_covered: true,
  expected_ms: 10000,
  population_revision: 1,
  revision: 1,
  explicit_limit_bytes: null,
  engine_capacity_bytes: null,
  ...patch,
});
const draw = (points: DockerHistoryPoint[]) =>
  buildSeriesRuns(points, "cpu", 0, 100_000, 1000, 100, 100);
describe("comparison point geometry", () => {
  it("joins only exact adjacent covered spans and positions real timestamps", () => {
    const result = draw([point(0), point(10000), point(30000)]);
    expect(result.runs).toHaveLength(1);
    expect(result.runs[0]?.map((p) => p.x)).toEqual([0, 100]);
    expect(result.dots[0]?.x).toBe(300);
  });
  it("draws a measured partial bucket and singleton as isolated six-pixel dot positions", () => {
    const result = draw([
      point(0),
      point(10000, { cpu_covered: false, cpu_measured_ms: 7000 }),
      point(20000),
    ]);
    expect(result.runs).toEqual([]);
    expect(result.dots.map((p) => p.partial)).toEqual([false, true, false]);
  });
  it("leaves gaps for absent weights and null values; measured zero is real", () => {
    const result = draw([
      point(0, { cpu_measured_ms: 0 }),
      point(10000, { metrics: { ...metrics, cpu_pct: null } }),
      point(20000, { metrics: { ...metrics, cpu_pct: 0 } }),
    ]);
    expect(result.dots.map((p) => p.value)).toEqual([0]);
  });
  it("uses per-metric quality and peaks only above the base tier", () => {
    const p = point(0, {
      cpu_measured_ms: 0,
      memory_measured_ms: 7000,
      memory_covered: false,
    });
    expect(pointValue(p, "cpu")).toBeNull();
    expect(pointValue(p, "memory")).not.toBeNull();
    expect(pointValue(point(0), "cpu")).toBe(10);
    expect(pointValue(point(0, { duration_ms: 60000 }), "cpu")).toBe(30);
  });
  it("does not bridge overlaps or extend outside the selected range", () => {
    const result = draw([point(-10000), point(0), point(5000), point(100000)]);
    expect(result.runs).toEqual([]);
    expect(result.dots).toHaveLength(2);
  });
});
describe("comparison membership", () => {
  const ranked = Array.from({ length: 7 }, (_, i) => ({
    id: String(i),
    value: 100 - i,
  }));
  const empty = { slots: {}, streaks: {} };
  it("ranks descending, breaking ties by id with missing values last", () =>
    expect(
      rankCandidates([
        { id: "z", value: null },
        { id: "b", value: 0 },
        { id: "a", value: 0 },
      ]).map((r) => r.id)
    ).toEqual(["a", "b", "z"]));
  it("selects all up to six and otherwise five in rank order", () => {
    expect(
      Object.keys(nextMembership(empty, ranked.slice(0, 6), true).slots)
    ).toHaveLength(6);
    expect(nextMembership(empty, ranked, true).slots).toEqual({
      0: 1,
      1: 2,
      2: 3,
      3: 4,
      4: 5,
    });
  });
  it("requires three consecutive challenger evaluations and keeps retained slots", () => {
    let state = nextMembership(empty, ranked, true);
    const changed = ranked.map((r) =>
      r.id === "6" ? { ...r, value: 200 } : r
    );
    state = nextMembership(state, changed, false);
    expect(state.slots["6"]).toBeUndefined();
    state = nextMembership(state, changed, false);
    expect(state.slots["6"]).toBeUndefined();
    state = nextMembership(state, changed, false);
    expect(state.slots["6"]).toBe(5);
    expect(state.slots["0"]).toBe(1);
  });
  it("resets interrupted streaks and skips focused or pinned incumbents", () => {
    let state = nextMembership(empty, ranked, true);
    const changed = ranked.map((r) =>
      r.id === "6" ? { ...r, value: 200 } : r
    );
    state = nextMembership(state, changed, false, ["4"]);
    state = nextMembership(state, ranked, false, ["4"]);
    state = nextMembership(state, changed, false, ["4"]);
    state = nextMembership(state, changed, false, ["4"]);
    expect(state.slots["6"]).toBeUndefined();
    state = nextMembership(state, changed, false, ["4"]);
    expect(state.slots["4"]).toBe(5);
    expect(state.slots["6"]).toBe(4);
  });
  it("recomputes immediately without reassigning retained colors and handles a population growing past six", () => {
    const initial = nextMembership(empty, ranked.slice(0, 6), true);
    const changed = nextMembership(initial, [...ranked].reverse(), false);
    expect(Object.keys(changed.slots)).toHaveLength(5);
    const switchMetric = nextMembership(
      changed,
      ranked.map((r) => ({ ...r, value: Number(r.id) })),
      true
    );
    expect(switchMetric.slots["4"]).toBe(5);
    expect(new Set(Object.values(switchMetric.slots)).size).toBe(5);
  });
});
