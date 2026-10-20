import { splitGaps } from "@core/chart-math";
import { scaleLinear } from "d3-scale";
import { area, line } from "d3-shape";
import { type ReactNode, useId } from "react";
import {
  annotationFraction,
  ChartAnnotation,
  type ChartAnnotationProps,
} from "./chart-annotation";
import { ChartFrame, ChartGrid, ChartSelectionVeil } from "./chart-frame";
import { GapBands, type GapSpan } from "./gap-band";
import { type Accent, accentVars, rampColor } from "./lib/accent";
import { useTickScroll } from "./lib/use-tick-scroll";

/** Width of the drawing in viewBox units; the SVG stretches to its box. */
const VB_W = 1000;
/** Annotations closer than this fraction of the width would overlap. */
const MIN_ANNOTATION_GAP = 0.1;

/**
 * Contract for the live area/line chart (design-system.md inventory). The
 * charts worker replaces the component body; keep the props shape.
 */
export interface StreamAreaProps {
  /** Series drawn back to front. `null` is a gap: the line breaks there. */
  series: {
    key: string;
    values: (number | null)[];
    /** Ramp step of the accent; step 1 gets the area fill. */
    step: 1 | 2 | 3 | 4;
    /**
     * Draw the area under the line (default). `false` is a bare line, as
     * for per-app lines over a total.
     */
    fill?: boolean;
    /** Hollow dots on the edges that border a gap (default true). */
    edges?: boolean;
    /** A dashed line, to tell apart two lines on the same ramp step. */
    dash?: boolean;
    /** Line width in px (default 1.5). */
    weight?: number;
    /** Opacity of the line and its area (default 1), for focus emphasis. */
    opacity?: number;
  }[];
  /** Timestamp of the last value (ms epoch). Values are `intervalMs` apart. */
  tEndMs: number;
  intervalMs: number;
  /** Y ceiling, already snapped (fixed 100, or a nice ceiling from the hook). */
  yMax: number;
  accent: Accent;
  height: number;
  ariaLabel: string;
  /** Corner label for the ceiling ("40%"), top left. */
  ceilingLabel?: string;
  /** Corner label for the window ("60s"), bottom right. */
  windowLabel?: string;
  /** Horizontal gridlines at these y values (the baseline is always drawn). */
  gridlines?: readonly number[];
  /** Y tick labels in a 32 px gutter on the left (100, 75, 50, 25). */
  yTicks?: readonly { value: number; label: string }[];
  /** X tick labels spread under the plot, oldest first ("−60s" … "now"). */
  xTicks?: readonly string[];
  /**
   * Labelled gaps (sleep, paused) drawn as hatched bands over the plot;
   * the parts outside the window are clipped.
   */
  gaps?: readonly GapSpan[];
  /**
   * A selected span as fractions of the plot width; the plot outside it is
   * veiled. Null or absent: nothing is selected.
   */
  highlight?: { left: number; width: number } | null;
  /** Drawn over the plot area, sized to it (a brush layer). */
  overlay?: ReactNode;
  /**
   * Labelled markers at the top of the plot (power spikes, D-083); ones
   * outside the window are dropped.
   */
  annotations?: readonly ChartAnnotationProps[];
}

interface Pt {
  x: number;
  y: number;
}

/**
 * Live area chart. Each series is split at `null` into
 * runs; nothing bridges a gap, and a hollow dot marks each edge that borders
 * one. A one-point run draws no line (a round cap would make it a stray
 * dot); its edge dot still marks it. New samples slide in with a
 * `translateX` on the plot group.
 */
export function StreamArea({
  series,
  tEndMs,
  intervalMs,
  yMax,
  accent,
  height,
  ariaLabel,
  ceilingLabel,
  windowLabel,
  gridlines = [],
  yTicks,
  xTicks,
  gaps,
  highlight,
  overlay,
  annotations,
}: StreamAreaProps) {
  const uid = useId();
  const n = Math.max(2, ...series.map((s) => s.values.length));
  const step = VB_W / (n - 1);
  const x = scaleLinear()
    .domain([0, n - 1])
    .range([0, VB_W]);
  const y = scaleLinear()
    .domain([0, yMax > 0 ? yMax : 1])
    .range([height - 1, 1])
    .clamp(true);
  const lineGen = line<Pt>()
    .x((p) => p.x)
    .y((p) => p.y);
  const areaGen = area<Pt>()
    .x((p) => p.x)
    .y0(height)
    .y1((p) => p.y);
  const scrollRef = useTickScroll<SVGGElement>(
    tEndMs,
    intervalMs,
    `${step.toFixed(2)}px`
  );

  const drawn = series.map((s) => {
    // Right-align: the last value sits at "now" even when the series is short.
    const offset = n - s.values.length;
    const runs = splitGaps(s.values).map((run) =>
      run.values.map((v, j) => ({
        x: x(offset + run.start + j),
        y: y(v),
      }))
    );
    const filled = s.fill !== false;
    const lines = runs.filter((pts) => pts.length > 1);
    const linePath = lines.map((pts) => lineGen(pts) ?? "").join("");
    const areaPath = filled
      ? lines.map((pts) => areaGen(pts) ?? "").join("")
      : "";
    const edges: Pt[] = [];
    (s.edges !== false ? runs : []).forEach((pts, k) => {
      const first = pts[0];
      const last = pts[pts.length - 1];
      if (k > 0 && first) edges.push(first);
      // A one-point run is its own first and last: one dot, not two with
      // the same key (React then leaves stale dots behind).
      if (k < runs.length - 1 && last && (k === 0 || pts.length > 1)) {
        edges.push(last);
      }
    });
    const stroke =
      s.step === 1 ? "var(--a-ink)" : rampColor(s.step, "var(--a-ink)");
    const fill = s.step === 1 ? "var(--a)" : rampColor(s.step);
    return { s, filled, linePath, areaPath, edges, stroke, fill };
  });
  const windowMs = (n - 1) * intervalMs;

  return (
    <ChartFrame
      height={height}
      ceiling={yMax}
      yTicks={yTicks}
      xTicks={xTicks}
      style={accentVars(accent)}
    >
      <svg
        viewBox={`0 0 ${VB_W} ${height}`}
        preserveAspectRatio="none"
        width="100%"
        height={height}
        role="img"
        aria-label={ariaLabel}
        className="block overflow-hidden"
      >
        <defs>
          {drawn
            .filter((d) => d.filled)
            .map(({ s, fill }) => (
              <linearGradient
                key={s.key}
                id={`${uid}-${s.key}`}
                x1="0"
                y1="0"
                x2="0"
                y2="1"
              >
                <stop
                  offset="0"
                  style={{ stopColor: fill, stopOpacity: 0.4 }}
                />
                <stop offset="1" style={{ stopColor: fill, stopOpacity: 0 }} />
              </linearGradient>
            ))}
        </defs>
        <ChartGrid values={gridlines} ceiling={yMax} height={height} />
        <g ref={scrollRef}>
          {drawn.map(({ s, filled, linePath, areaPath, stroke }) => (
            <g key={s.key} data-series={s.key} opacity={s.opacity ?? 1}>
              {filled && (
                <path data-area d={areaPath} fill={`url(#${uid}-${s.key})`} />
              )}
              <path
                data-line
                d={linePath}
                fill="none"
                style={{ stroke }}
                strokeWidth={s.weight ?? 1.5}
                strokeDasharray={s.dash ? "3 2" : undefined}
                strokeLinecap="round"
                strokeLinejoin="round"
                vectorEffect="non-scaling-stroke"
              />
            </g>
          ))}
        </g>
      </svg>
      {gaps && gaps.length > 0 && tEndMs > 0 && (
        <GapBands
          gaps={gaps}
          rangeFromMs={tEndMs - windowMs}
          rangeToMs={tEndMs}
        />
      )}
      <ChartSelectionVeil highlight={highlight} />
      {drawn.flatMap(({ s, edges, stroke }) =>
        edges.map((p) => (
          <span
            key={`${s.key}-${p.x}`}
            data-gap-edge
            aria-hidden
            className="absolute -mt-[3.5px] -ml-[3.5px] size-[7px] rounded-full border-[1.5px] bg-card"
            style={{
              left: `${(p.x / VB_W) * 100}%`,
              top: p.y,
              borderColor: stroke,
            }}
          />
        ))
      )}
      {placeAnnotations(annotations, tEndMs, windowMs).map(({ a, f }) => {
        // Past the middle the label hangs left of its marker, so it stays
        // inside the plot.
        const end = f > 0.5;
        return (
          <div
            key={`${a.tsMs}-${a.label}`}
            className="absolute top-3"
            style={
              end
                ? { right: `${((1 - f) * 100).toFixed(2)}%` }
                : { left: `${(f * 100).toFixed(2)}%` }
            }
          >
            <ChartAnnotation {...a} align={end ? "end" : "start"} />
          </div>
        );
      })}
      {ceilingLabel && (
        <span
          aria-hidden
          className="figures absolute -top-0.5 left-0 text-[9px] text-fg-faint"
        >
          {ceilingLabel}
        </span>
      )}
      {windowLabel && (
        <span
          aria-hidden
          className="figures absolute right-0 -bottom-3.5 text-[9px] text-fg-faint"
        >
          {windowLabel}
        </span>
      )}
      {overlay}
    </ChartFrame>
  );
}

/**
 * Annotations inside the window with their fractions, newest first. One
 * closer than `MIN_ANNOTATION_GAP` to a newer one is dropped, so labels of
 * spikes close together don't overlap.
 */
function placeAnnotations(
  annotations: readonly ChartAnnotationProps[] | undefined,
  tEndMs: number,
  windowMs: number
): { a: ChartAnnotationProps; f: number }[] {
  const placed: { a: ChartAnnotationProps; f: number }[] = [];
  const newest = [...(annotations ?? [])].sort((p, q) => q.tsMs - p.tsMs);
  for (const a of newest) {
    const f = annotationFraction(a.tsMs, tEndMs, windowMs);
    if (f === null) continue;
    if (placed.some((p) => Math.abs(p.f - f) < MIN_ANNOTATION_GAP)) continue;
    placed.push({ a, f });
  }
  return placed;
}
