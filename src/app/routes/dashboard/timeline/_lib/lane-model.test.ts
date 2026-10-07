import type { Bucket } from "./buckets";
import { buildLaneModel } from "./lane-model";
import { LANES } from "./lanes";

const MIN = 60_000;
const HOUR = 60 * MIN;
const BUCKET = 30 * MIN;
const CPU = LANES.find((l) => l.id === "cpu");
if (!CPU) throw new Error("no CPU lane");

/** 30-minute buckets over two days with the gaps' slots left out. */
function series(gaps: { fromMs: number; toMs: number }[]): Bucket[] {
  const out: Bucket[] = [];
  for (let t = 0; t < 48 * HOUR; t += BUCKET) {
    if (gaps.some((g) => t + BUCKET > g.fromMs && t < g.toMs)) continue;
    out.push({ t, avg: 20, min: 10, max: 30 });
  }
  return out;
}

describe("buildLaneModel gap edges on long ranges", () => {
  // A night (6 h) and a weekend-sized hole (20 h).
  const night = { fromMs: 6 * HOUR, toMs: 12 * HOUR, label: "night" };
  const away = { fromMs: 24 * HOUR, toMs: 44 * HOUR, label: "away" };
  const gaps = [night, away];
  const buckets = { "cpu.total": series(gaps) };
  const HOLDS = { "cpu.total": BUCKET };

  it("puts hollow dots on both sides of every gap by default", () => {
    const model = buildLaneModel(CPU, buckets, HOLDS, BUCKET, gaps);
    expect(model.series[0]?.edges).toHaveLength(4);
  });

  it("leaves narrow gaps to their band alone", () => {
    const model = buildLaneModel(CPU, buckets, HOLDS, BUCKET, gaps, 12 * HOUR);
    const x = model.x;
    const edges = (model.series[0]?.edges ?? []).map((i) => x[i] as number);
    // Only the 20-hour hole keeps its dots.
    expect(edges).toEqual([away.fromMs - BUCKET, away.toMs]);
    // The line still breaks at the night: a null slot inside it.
    const avg = model.series[0]?.cols.avg ?? [];
    const nightSlot = x.findIndex((t) => t >= night.fromMs && t < night.toMs);
    expect(nightSlot).toBeGreaterThan(-1);
    expect(avg[nightSlot]).toBeNull();
  });
});
