import type { DockerHistoryPoint } from "@core/generated/bindings";

export type ComparisonMetric = "cpu" | "memory";
export interface SeriesVertex {
  x: number;
  y: number;
  time: number;
  value: number;
  partial: boolean;
}
export function pointValue(
  point: DockerHistoryPoint,
  metric: ComparisonMetric
): number | null {
  const measured =
    metric === "cpu" ? point.cpu_measured_ms : point.memory_measured_ms;
  if (!measured) return null;
  const values = point.duration_ms > 10_000 ? point.peak : point.metrics;
  return metric === "cpu" ? values.cpu_pct : values.working_set_bytes;
}
export function pointCovered(
  point: DockerHistoryPoint,
  metric: ComparisonMetric
) {
  return metric === "cpu" ? point.cpu_covered : point.memory_covered;
}
/** Timestamp geometry only: quality and metric semantics arrive from Rust (R-11/R-46). */
export function buildSeriesRuns(
  points: readonly DockerHistoryPoint[],
  metric: ComparisonMetric,
  from: number,
  to: number,
  width: number,
  height: number,
  ceiling: number
) {
  const runs: SeriesVertex[][] = [];
  const dots: SeriesVertex[] = [];
  let run: SeriesVertex[] = [];
  let previousEnd: number | null = null;
  const flush = () => {
    if (run.length === 1 && run[0]) dots.push(run[0]);
    else if (run.length > 1) runs.push(run);
    run = [];
  };
  for (const point of points) {
    if (point.time_ms < from || point.time_ms >= to) continue;
    const value = pointValue(point, metric);
    if (value === null) {
      flush();
      previousEnd = null;
      continue;
    }
    const partial = !pointCovered(point, metric);
    const vertex = {
      x: ((point.time_ms - from) / Math.max(1, to - from)) * width,
      y: height - (value / ceiling) * height,
      time: point.time_ms,
      value,
      partial,
    };
    if (partial) {
      flush();
      dots.push(vertex);
      previousEnd = null;
      continue;
    }
    if (previousEnd !== point.time_ms) flush();
    run.push(vertex);
    previousEnd = point.time_ms + point.duration_ms;
  }
  flush();
  return { runs, dots };
}
export interface RankedCandidate {
  id: string;
  value: number | null;
}
export function rankCandidates<T extends RankedCandidate>(
  candidates: readonly T[]
): T[] {
  return [...candidates].sort(
    (a, b) =>
      (b.value ?? -Infinity) - (a.value ?? -Infinity) ||
      a.id.localeCompare(b.id)
  );
}
export interface Membership {
  slots: Record<string, number>;
  streaks: Record<string, number>;
}
/** Stable slots survive re-ranking; three live evaluations are required for a replacement. */
export function nextMembership(
  previous: Membership,
  candidates: readonly RankedCandidate[],
  immediate: boolean,
  protectedIds: readonly string[] = []
): Membership {
  const ranked = rankCandidates(candidates);
  const count = ranked.length <= 6 ? ranked.length : 5;
  const available = new Set(ranked.map((row) => row.id));
  const slots = Object.fromEntries(
    Object.entries(previous.slots).filter(([id]) => available.has(id))
  );
  const streaks: Record<string, number> = {};
  if (immediate || ranked.length <= 6) {
    const protectedRows = ranked.filter(
      (row) => protectedIds.includes(row.id) && row.id in slots
    );
    const desired = new Set(
      [
        ...protectedRows,
        ...ranked.filter((row) => !protectedRows.includes(row)),
      ]
        .slice(0, count)
        .map((row) => row.id)
    );
    for (const id of Object.keys(slots)) if (!desired.has(id)) delete slots[id];
    for (const row of ranked.filter((r) => desired.has(r.id)))
      if (!(row.id in slots)) slots[row.id] = freeSlot(slots);
    return { slots, streaks };
  }
  while (Object.keys(slots).length > count) {
    const lowest = [...ranked]
      .reverse()
      .find((r) => r.id in slots && !protectedIds.includes(r.id));
    if (!lowest) break;
    delete slots[lowest.id];
  }
  for (const row of ranked) {
    if (row.id in slots) continue;
    if (Object.keys(slots).length < count) {
      slots[row.id] = freeSlot(slots);
      continue;
    }
    const lowest = [...ranked]
      .reverse()
      .find((r) => r.id in slots && !protectedIds.includes(r.id));
    if (!lowest || ranked.indexOf(row) >= ranked.indexOf(lowest)) continue;
    const key = `${row.id}:${lowest.id}`;
    streaks[key] = (previous.streaks[key] ?? 0) + 1;
    if (streaks[key] >= 3) {
      delete slots[lowest.id];
      slots[row.id] = freeSlot(slots);
      delete streaks[key];
    }
  }
  return { slots, streaks };
}
function freeSlot(slots: Record<string, number>) {
  const occupied = new Set(Object.values(slots));
  return [1, 2, 3, 4, 5, 6].find((slot) => !occupied.has(slot)) ?? 6;
}
export const seriesColor = (slot: number) => `var(--color-series-${slot})`;
