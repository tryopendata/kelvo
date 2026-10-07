import { splitGaps } from "@core/chart-math";
import { scaleLinear } from "d3-scale";
import { area, line } from "d3-shape";
import { useId } from "react";
import { cn } from "~/lib/utils";
import { GapBands, type GapSpan } from "./gap-band";
import { type Accent, accentVars, rampColor } from "./lib/accent";
import { useTickScroll } from "./lib/use-tick-scroll";

/** Width of the drawing in viewBox units; the SVG stretches to its box. */
const VB_W = 1000;

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
  gridlines?: number[];
  /** Y tick labels in a 32 px gutter on the left (100, 75, 50, 25). */
  yTicks?: { value: number; label: string }[];
  /** X tick labels spread under the plot, oldest first ("−60s" … "now"). */
  xTicks?: string[];
  /**
   * Labelled gaps (sleep, paused) drawn as hatched bands over the plot;
   * the parts outside the window are clipped.
   */
  gaps?: readonly GapSpan[];
}

interface Pt {
  x: number;
  y: number;
}

/**
 * Live area chart. Each series is split at `null` into
 * runs; nothing bridges a gap, and a hollow dot marks each edge that borders
 * one. New samples slide in with a `translateX` on the plot group.
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
    const linePath = runs.map((pts) => lineGen(pts) ?? "").join("");
    const areaPath = runs.map((pts) => areaGen(pts) ?? "").join("");
    const edges: Pt[] = [];
    runs.forEach((pts, k) => {
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
    return { s, linePath, areaPath, edges, stroke, fill };
  });

  return (
    <div
      className={cn("flex flex-col gap-1.5", yTicks && "pl-8")}
      style={accentVars(accent)}
    >
      <div className="relative" style={{ height }}>
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
            {drawn.map(({ s, fill }) => (
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
          {gridlines.length > 0 && (
            <path
              d={gridlines.map((g) => `M0 ${y(g).toFixed(1)}H${VB_W}`).join("")}
              stroke="var(--color-grid)"
              vectorEffect="non-scaling-stroke"
            />
          )}
          <path
            d={`M0 ${height - 0.5}H${VB_W}`}
            stroke="var(--color-axis)"
            vectorEffect="non-scaling-stroke"
          />
          <g ref={scrollRef}>
            {drawn.map(({ s, linePath, areaPath, stroke }) => (
              <g key={s.key} data-series={s.key}>
                <path d={areaPath} fill={`url(#${uid}-${s.key})`} />
                <path
                  data-line
                  d={linePath}
                  fill="none"
                  style={{ stroke }}
                  strokeWidth={1.5}
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
            rangeFromMs={tEndMs - (n - 1) * intervalMs}
            rangeToMs={tEndMs}
          />
        )}
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
        {yTicks?.map((t) => (
          <span
            key={t.label}
            aria-hidden
            className="data-mono absolute -left-8 -translate-y-1/2 text-[10px] text-fg-faint"
            style={{ top: y(t.value) }}
          >
            {t.label}
          </span>
        ))}
        {ceilingLabel && (
          <span
            aria-hidden
            className="data-mono absolute -top-0.5 left-0 text-[9px] text-fg-faint"
          >
            {ceilingLabel}
          </span>
        )}
        {windowLabel && (
          <span
            aria-hidden
            className="data-mono absolute right-0 -bottom-3.5 text-[9px] text-fg-faint"
          >
            {windowLabel}
          </span>
        )}
      </div>
      {xTicks && xTicks.length > 0 && (
        <div aria-hidden className="flex justify-between">
          {xTicks.map((t, i) => (
            <span
              key={t}
              className={cn(
                "data-mono text-[10px]",
                i === xTicks.length - 1
                  ? "text-muted-foreground"
                  : "text-fg-faint"
              )}
            >
              {t}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
