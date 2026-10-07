import { scaleLinear } from "d3-scale";
import { area, line } from "d3-shape";
import { useId } from "react";
import { cn } from "~/lib/utils";
import {
  annotationFraction,
  ChartAnnotation,
  type ChartAnnotationProps,
} from "./chart-annotation";
import { GapBands, type GapSpan } from "./gap-band";
import { accentVars } from "./lib/accent";
import { windowTicks } from "./lib/chart-labels";
import { useTickScroll } from "./lib/use-tick-scroll";

const VB_W = 1000;

export type PowerComponent = "cpu" | "gpu" | "ane" | "dram";

export interface PowerStackProps {
  /** Watts per component, oldest first, `intervalMs` apart. `null` is a gap. */
  series: { key: PowerComponent; values: (number | null)[] }[];
  intervalMs: number;
  /** Timestamp of the last sample (ms epoch). */
  tEndMs: number;
  /** Y ceiling in watts, already snapped to a nice step. */
  yMax: number;
  height?: number;
  annotations: ChartAnnotationProps[];
  ariaLabel?: string;
  /** Labelled gaps drawn as hatched bands over the plot. */
  gaps?: readonly GapSpan[];
}

/** Bottom to top; CPU on top carries the outline. */
const ORDER: PowerComponent[] = ["dram", "ane", "gpu", "cpu"];
const OPACITY: Record<PowerComponent, number> = {
  dram: 0.22,
  ane: 1,
  gpu: 0.5,
  cpu: 0.85,
};

interface Band {
  i: number;
  lo: number;
  hi: number;
}

/**
 * Stacked power by component over the last minutes, in the Power
 * accent: CPU, GPU and DRAM as ramp steps, ANE as a hatch because it is
 * usually zero. A tick where any component is missing breaks the whole stack;
 * a partial stack would misstate the total.
 */
export function PowerStack({
  series,
  intervalMs,
  tEndMs,
  yMax,
  height = 200,
  annotations,
  ariaLabel = "Stacked power: CPU, GPU, ANE, DRAM",
  gaps,
}: PowerStackProps) {
  const uid = useId();
  const byKey = new Map(series.map((s) => [s.key, s.values]));
  const present = ORDER.filter((k) => byKey.has(k));
  const n = Math.max(2, ...series.map((s) => s.values.length));
  const windowMs = (n - 1) * intervalMs;
  const x = scaleLinear()
    .domain([0, n - 1])
    .range([0, VB_W]);
  const y = scaleLinear()
    .domain([0, yMax > 0 ? yMax : 1])
    .range([height, 0])
    .clamp(true);
  const complete = (i: number) =>
    present.every((k) => {
      const v = byKey.get(k)?.[i];
      return v != null && Number.isFinite(v);
    });

  const cumulative = new Array<number>(n).fill(0);
  const bands = present.map((k) => {
    const values = byKey.get(k) ?? [];
    const pts: (Band | null)[] = [];
    for (let i = 0; i < n; i++) {
      if (!complete(i)) {
        pts.push(null);
        continue;
      }
      const lo = cumulative[i] ?? 0;
      const hi = lo + (values[i] ?? 0);
      cumulative[i] = hi;
      pts.push({ i, lo, hi });
    }
    return { key: k, pts };
  });

  const areaGen = area<Band | null>()
    .defined((p) => p !== null)
    .x((p) => x(p?.i ?? 0))
    .y0((p) => y(p?.lo ?? 0))
    .y1((p) => y(p?.hi ?? 0));
  const topGen = line<Band | null>()
    .defined((p) => p !== null)
    .x((p) => x(p?.i ?? 0))
    .y((p) => y(p?.hi ?? 0));
  const top = bands[bands.length - 1];
  const scrollRef = useTickScroll<SVGGElement>(
    tEndMs,
    intervalMs,
    `${(VB_W / (n - 1)).toFixed(2)}px`
  );
  const ticks = windowTicks(windowMs, 3);

  return (
    <div className="flex flex-col gap-1.5 pl-6.5" style={accentVars("power")}>
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
            <pattern
              id={`${uid}-ane`}
              width="6"
              height="6"
              patternUnits="userSpaceOnUse"
              patternTransform="rotate(45)"
            >
              <rect
                width="2"
                height="6"
                style={{ fill: "var(--a)", fillOpacity: 0.7 }}
              />
            </pattern>
          </defs>
          <path
            d={[0, 0.25, 0.5, 0.75]
              .map(
                (f) =>
                  `M0 ${(f * height + (f === 0 ? 0.5 : 0)).toFixed(1)}H${VB_W}`
              )
              .join("")}
            stroke="var(--color-grid)"
            vectorEffect="non-scaling-stroke"
          />
          <path
            d={`M0 ${height - 0.5}H${VB_W}`}
            stroke="var(--color-axis)"
            vectorEffect="non-scaling-stroke"
          />
          <g ref={scrollRef}>
            {bands.map((b) => (
              <path
                key={b.key}
                data-component={b.key}
                d={areaGen(b.pts) ?? ""}
                style={
                  b.key === "ane"
                    ? { fill: `url(#${uid}-ane)` }
                    : { fill: "var(--a)", fillOpacity: OPACITY[b.key] }
                }
              />
            ))}
            {top && (
              <path
                data-line
                d={topGen(top.pts) ?? ""}
                fill="none"
                stroke="var(--a-ink)"
                strokeWidth={1.5}
                strokeLinecap="round"
                strokeLinejoin="round"
                vectorEffect="non-scaling-stroke"
              />
            )}
          </g>
        </svg>
        {gaps && gaps.length > 0 && tEndMs > 0 && (
          <GapBands
            gaps={gaps}
            rangeFromMs={tEndMs - windowMs}
            rangeToMs={tEndMs}
          />
        )}
        <span
          aria-hidden
          className="data-mono absolute -top-1.5 -left-6.5 text-[10px] text-fg-faint"
        >
          {`${yMax}W`}
        </span>
        <span
          aria-hidden
          className="data-mono absolute -left-6.5 -translate-y-1/2 text-[10px] text-fg-faint"
          style={{ top: height / 2 }}
        >
          {yMax / 2}
        </span>
        {annotations.map((a) => {
          const f = annotationFraction(a.tsMs, tEndMs, windowMs);
          if (f === null) return null;
          return (
            <div
              key={`${a.tsMs}-${a.label}`}
              className="absolute top-3"
              style={{ left: `${(f * 100).toFixed(2)}%` }}
            >
              <ChartAnnotation {...a} />
            </div>
          );
        })}
      </div>
      <div aria-hidden className="flex justify-between">
        {ticks.map((t, i) => (
          <span
            // A window under 2 s gives two "−1s" labels; position is the identity.
            // biome-ignore lint/suspicious/noArrayIndexKey: fixed tick slots
            key={i}
            className={cn(
              "data-mono text-[10px]",
              i === ticks.length - 1 ? "text-muted-foreground" : "text-fg-faint"
            )}
          >
            {t}
          </span>
        ))}
      </div>
    </div>
  );
}
