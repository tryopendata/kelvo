/**
 * What one lane draws: its y domain (plan 4.6 table), the plotted series as
 * aligned columns, and the mapping from a value to a height in the lane for
 * the crosshair dots.
 */
import { niceCeiling } from "@core/chart-math";
import type { Accent } from "~/widgets/lib/accent";
import {
  alignColumns,
  type Bucket,
  type Column,
  type GapSpan,
  gapEdgeIndices,
  maxOf,
  type SeriesColumns,
} from "./buckets";
import type { LaneDef } from "./lanes";

export interface PlotSeries {
  metric: string;
  cols: SeriesColumns;
  accent: Accent;
  look: "area" | "line" | "faint";
  envelope: boolean;
  direction?: "up" | "down";
  edges: number[];
}

export interface LaneModel {
  def: LaneDef;
  x: number[];
  series: PlotSeries[];
  domain: [number, number];
  /** Where area fills end. */
  baseline: number;
  /** Fraction of the lane's height from the top for `value` of `metric`. */
  yFraction: (metric: string, value: number) => number;
}

/** Temperature lane domain in °C (plan 4.6). */
export const TEMP_DOMAIN: [number, number] = [30, 100];

const clamp01 = (f: number) => Math.min(1, Math.max(0, f));

function scaleCols(cols: SeriesColumns, k: number): SeriesColumns {
  const s = (c: Column) => c.map((v) => (v == null ? v : v * k));
  // A negative scale flips the order: the band still spans min to max.
  return k < 0
    ? { min: s(cols.max), max: s(cols.min), avg: s(cols.avg) }
    : { min: s(cols.min), max: s(cols.max), avg: s(cols.avg) };
}

export function buildLaneModel(
  def: LaneDef,
  series: Record<string, Bucket[]>,
  /** Each metric's `hold_ms`: how far apart its points still join. */
  holds: Record<string, number>,
  bucketMs: number,
  gaps: readonly GapSpan[],
  /**
   * Gaps shorter than this get no hollow dots at their edges. On 30d a
   * night's sleep is a few pixels wide, and a dot on each side of every
   * night would bead the whole line; the band alone marks it.
   */
  minEdgeGapMs = 0
): LaneModel {
  const plotted = def.metrics.filter((m) => m.plotted);
  const aligned = alignColumns(
    plotted.map((m) => series[m.metric] ?? []),
    plotted.map((m) => holds[m.metric] ?? bucketMs),
    bucketMs,
    gaps
  );
  const colsOf = (i: number) =>
    aligned.series[i] ?? { min: [], max: [], avg: [] };
  const dotted = gaps.filter((g) => g.toMs - g.fromMs >= minEdgeGapMs);
  const holdOf = (i: number) => holds[plotted[i]?.metric ?? ""] ?? bucketMs;
  const edges = (i: number, cols = colsOf(i)) =>
    gapEdgeIndices(aligned.x, cols.avg, dotted, bucketMs, holdOf(i));

  const accent = def.accent;
  let domain: [number, number] = [0, 100];
  let baseline = 0;
  let out: PlotSeries[];
  const scale: Record<string, number> = {};

  switch (def.id) {
    case "power": {
      const ceiling = niceCeiling(maxOf(series["power.system"] ?? []) ?? 0, 5);
      domain = [0, ceiling];
      out = plotted.map((m, i) => ({
        metric: m.metric,
        cols: colsOf(i),
        accent,
        look: m.metric === "power.system" ? "faint" : "area",
        envelope: m.metric !== "power.system",
        edges: edges(i),
      }));
      break;
    }
    case "temp":
      domain = TEMP_DOMAIN;
      baseline = TEMP_DOMAIN[0];
      out = plotted.map((m, i) => ({
        metric: m.metric,
        cols: colsOf(i),
        accent,
        look: "line",
        envelope: true,
        edges: edges(i),
      }));
      break;
    case "network": {
      // Each half has its own nice ceiling; both map onto [-1, 1].
      domain = [-1, 1];
      out = plotted.map((m, i) => {
        const up = m.metric === "net.tx_total";
        const ceiling = niceCeiling(maxOf(series[m.metric] ?? []) ?? 0, 1000);
        const k = (up ? 1 : -1) / ceiling;
        scale[m.metric] = k;
        const cols = scaleCols(colsOf(i), k);
        return {
          metric: m.metric,
          cols,
          accent,
          look: "area",
          envelope: true,
          direction: up ? "up" : "down",
          edges: edges(i, cols),
        };
      });
      break;
    }
    default:
      out = plotted.map((m, i) => ({
        metric: m.metric,
        cols: colsOf(i),
        accent,
        look: "area",
        envelope: true,
        edges: edges(i),
      }));
  }

  const [lo, hi] = domain;
  return {
    def,
    x: aligned.x,
    series: out,
    domain,
    baseline,
    yFraction: (metric, value) =>
      clamp01((hi - value * (scale[metric] ?? 1)) / (hi - lo)),
  };
}
