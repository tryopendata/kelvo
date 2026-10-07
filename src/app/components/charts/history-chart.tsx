import { useEffect, useRef, useState } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { GapBand } from "~/widgets/gap-band";
import { type Accent, accentVars } from "~/widgets/lib/accent";

export interface HistoryPointProps {
  t: number;
  min: number | null;
  max: number | null;
  avg: number | null;
}

export interface HistoryChartProps {
  /** Buckets in time order (ms epoch). A missing value is `null`, never 0. */
  points: HistoryPointProps[];
  /** Spans with no data (sleep, paused). Lines break at both edges. */
  gaps: { fromMs: number; toMs: number; label: string }[];
  /** The x range shown. */
  range: { fromMs: number; toMs: number };
  /** Fixed y domain: [0, 100] for load, [30, 100] for temperature. */
  domain: [number, number];
  accent: Accent;
  height: number;
  ariaLabel: string;
}

type Column = (number | null)[];

/**
 * uPlot data for the avg line and the min/max envelope. Points inside a gap
 * are dropped and a `null` row is put at each gap's start, so neither the line
 * nor the band is drawn across it (`spanGaps` stays off).
 */
export function historyColumns(
  points: HistoryPointProps[],
  gaps: { fromMs: number; toMs: number }[]
): [number[], Column, Column, Column] {
  const inGap = (t: number) => gaps.some((g) => t >= g.fromMs && t < g.toMs);
  const rows: HistoryPointProps[] = points.filter((p) => !inGap(p.t));
  for (const g of gaps)
    rows.push({ t: g.fromMs, min: null, max: null, avg: null });
  rows.sort((a, b) => a.t - b.t);
  return [
    rows.map((r) => r.t),
    rows.map((r) => r.max),
    rows.map((r) => r.min),
    rows.map((r) => r.avg),
  ];
}

function cssColor(el: Element, name: string, alpha = 1): string {
  const raw = getComputedStyle(el).getPropertyValue(name).trim();
  const hex = /^#([0-9a-f]{6})$/i.exec(raw)?.[1];
  if (!hex) return raw || "currentColor";
  if (alpha >= 1) return raw;
  const n = Number.parseInt(hex, 16);
  return `rgba(${n >> 16}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

/** Re-render when `.dark` toggles on <html>: canvas colors are resolved once. */
function useThemeVersion(): number {
  const [version, setVersion] = useState(0);
  useEffect(() => {
    const observer = new MutationObserver(() => setVersion((v) => v + 1));
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["class"],
    });
    return () => observer.disconnect();
  }, []);
  return version;
}

/**
 * History chart on uPlot (canvas): avg line in the accent ink plus a 20%
 * min/max envelope, so a 1 s spike is not hidden by averaging. Gaps are
 * drawn as hatched GapBands over the plot and break the line. No axes: the
 * Timeline draws one shared x axis for every lane.
 */
export function HistoryChart({
  points,
  gaps,
  range,
  domain,
  accent,
  height,
  ariaLabel,
}: HistoryChartProps) {
  const plotRef = useRef<HTMLDivElement>(null);
  const theme = useThemeVersion();
  const [lo, hi] = domain;

  useEffect(() => {
    const el = plotRef.current;
    if (!el) return;
    // Read after the theme flip so the tokens resolve to the new values.
    void theme;
    const stroke = cssColor(el, "--a-ink");
    const band = cssColor(el, `--color-${accent}`, 0.2);
    const width = Math.max(
      1,
      el.clientWidth || el.getBoundingClientRect().width
    );
    const plot = new uPlot(
      {
        width,
        height,
        legend: { show: false },
        cursor: { show: false },
        scales: {
          x: { time: false, range: [range.fromMs, range.toMs] },
          y: { range: [lo, hi] },
        },
        axes: [{ show: false }, { show: false }],
        series: [
          {},
          { stroke: "transparent", points: { show: false } },
          { stroke: "transparent", points: { show: false } },
          { stroke, width: 1.5, points: { show: false } },
        ],
        bands: [{ series: [1, 2], fill: band }],
      },
      historyColumns(points, gaps),
      el
    );
    const resize =
      typeof ResizeObserver === "undefined"
        ? null
        : new ResizeObserver(([entry]) => {
            const w = entry?.contentRect.width;
            if (w) plot.setSize({ width: w, height });
          });
    resize?.observe(el);
    return () => {
      resize?.disconnect();
      plot.destroy();
    };
  }, [points, gaps, range.fromMs, range.toMs, lo, hi, accent, height, theme]);

  return (
    <div
      role="img"
      aria-label={ariaLabel}
      className="relative"
      style={{ ...accentVars(accent), height }}
    >
      <div ref={plotRef} data-history-plot className="absolute inset-0" />
      {gaps.map((g) => (
        <GapBand
          key={g.fromMs}
          fromMs={g.fromMs}
          toMs={g.toMs}
          label={g.label}
          rangeFromMs={range.fromMs}
          rangeToMs={range.toMs}
        />
      ))}
    </div>
  );
}
