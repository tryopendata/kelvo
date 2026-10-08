import type {
  TrayLayout,
  TrayMarker,
  TrayOwnItem,
  TrayReadout,
} from "@core/tray-layout";
import { Fragment, type ReactNode } from "react";
import { cn } from "~/lib/utils";

export interface TrayPreviewProps {
  /** What to draw (`trayLayout` over the settings and current values). */
  layout: TrayLayout;
  /** Clock text after the items, as the real menu bar shows it. */
  clock?: string;
  className?: string;
}

const BAR_H = 14;

function barHeight(pct: number | null): number {
  if (pct == null || !Number.isFinite(pct)) return 0;
  // Quantized to the pixel like the Rust renderer; any load shows a sliver.
  const h = Math.round((Math.min(100, Math.max(0, pct)) / 100) * BAR_H * 2) / 2;
  return pct > 0 ? Math.max(1, h) : 0;
}

/** The combined item's bars: 3 pt wide, 4 pt apart, 14 pt tall, 30% track. */
function BarsIcon({ bars }: { bars: readonly (number | null)[] }) {
  const width = bars.length * 7 - 4;
  return (
    <svg
      width={width}
      height="14"
      viewBox={`0 0 ${width} 14`}
      aria-hidden
      className="shrink-0"
    >
      <g fill="currentColor">
        {bars.map((v, i) => {
          const h = barHeight(v);
          const x = i * 7;
          return (
            // biome-ignore lint/suspicious/noArrayIndexKey: bars in fixed CPU, GPU, Memory order
            <Fragment key={i}>
              <rect x={x} y={0} width={3} height={BAR_H} rx={1} opacity={0.3} />
              {h > 0 && (
                <rect x={x} y={BAR_H - h} width={3} height={h} rx={1} />
              )}
            </Fragment>
          );
        })}
      </g>
    </svg>
  );
}

/** Power's bolt: the sidebar's power path squeezed into 7 x 12 pt. */
function BoltIcon() {
  return (
    <svg
      width="7"
      height="12"
      viewBox="3 2 18 20"
      preserveAspectRatio="none"
      aria-hidden
      className="shrink-0"
    >
      <path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z" fill="currentColor" />
    </svg>
  );
}

/** Disk used: an 11 x 7 pt drive with its dot. */
function DriveIcon() {
  return (
    <svg
      width="11"
      height="7"
      viewBox="0 0 11 7"
      aria-hidden
      className="shrink-0"
    >
      <rect
        x={0.6}
        y={0.6}
        width={9.8}
        height={5.8}
        rx={1.5}
        fill="none"
        stroke="currentColor"
        strokeWidth={1.2}
      />
      <circle cx={8.25} cy={4.25} r={0.75} fill="currentColor" />
    </svg>
  );
}

/** Three letters stacked one per line, 6 px mono, as the menu bar labels values. */
function Stacked({ text }: { text: string }) {
  return (
    <span
      aria-hidden
      className="inline-flex w-1.5 shrink-0 flex-col text-center font-tray text-[6px] leading-[6px]"
    >
      {text.split("").map((c, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: characters of a fixed label
        <span key={i}>{c}</span>
      ))}
    </span>
  );
}

/** A readout's marker exactly as the menu bar draws it; nothing for "61°". */
export function TrayMarkerIcon({ marker }: { marker: TrayMarker }) {
  if (marker.kind === "label") return <Stacked text={marker.text} />;
  if (marker.kind === "glyph") {
    return marker.glyph === "bolt" ? <BoltIcon /> : <DriveIcon />;
  }
  return null;
}

/** Network up over down, two lines of 8 px text, right-aligned. */
function Rates({ up, down }: { up: string; down: string }) {
  return (
    <span className="inline-flex shrink-0 flex-col items-end whitespace-pre font-tray text-[8px] leading-[9px]">
      <span>{up}</span>
      <span>{down}</span>
    </span>
  );
}

const Value = ({ children }: { children: ReactNode }) => (
  <span className="whitespace-nowrap font-tray text-[11px]">{children}</span>
);

function Readout({ r }: { r: TrayReadout }) {
  if (r.kind === "rates") return <Rates up={r.up} down={r.down} />;
  return (
    <span
      className={cn(
        "inline-flex items-center",
        r.marker.kind === "glyph" ? "gap-[3px]" : "gap-1"
      )}
    >
      <TrayMarkerIcon marker={r.marker} />
      <Value>{r.text}</Value>
    </span>
  );
}

/** Samples across the sparkline, as the Rust renderer draws it. */
export const SPARK_SAMPLES = 20;
/** Bars in the GPU history graph. */
export const HIST_SAMPLES = 9;

/**
 * Sparkline path in a 32 x 16 box: sample i at x 2 + 1.47 i, 0 to
 * 100% over y 14 to 2, newest at the right. A gap starts a new segment.
 */
export function sparkPath(samples: readonly (number | null)[]): string {
  const recent = samples.slice(-SPARK_SAMPLES);
  const offset = SPARK_SAMPLES - recent.length;
  let d = "";
  let open = false;
  recent.forEach((v, i) => {
    if (v == null || !Number.isFinite(v)) {
      open = false;
      return;
    }
    const x = (2 + (offset + i) * 1.47).toFixed(1);
    const y = (14 - (Math.min(100, Math.max(0, v)) / 100) * 12).toFixed(1);
    d += `${open ? "L" : "M"}${x} ${y} `;
    open = true;
  });
  return d.trim();
}

const BoxOutline = () => (
  <rect
    x={0.5}
    y={0.5}
    width={31}
    height={15}
    rx={2}
    fill="none"
    stroke="currentColor"
    opacity={0.35}
  />
);

/** CPU line in its box. */
function SparkIcon({ samples }: { samples: readonly (number | null)[] }) {
  return (
    <svg width="32" height="16" viewBox="0 0 32 16" aria-hidden>
      <BoxOutline />
      <path
        d={sparkPath(samples)}
        fill="none"
        stroke="currentColor"
        strokeWidth={1.2}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}

/** GPU history: the last 9 samples as 2 pt bars on a 3 pt pitch. */
function HistIcon({ samples }: { samples: readonly (number | null)[] }) {
  const recent = samples.slice(-HIST_SAMPLES);
  const offset = HIST_SAMPLES - recent.length;
  return (
    <svg width="32" height="16" viewBox="0 0 32 16" aria-hidden>
      <BoxOutline />
      {recent.map((v, i) => {
        if (v == null || !Number.isFinite(v)) return null;
        const h = (Math.min(100, Math.max(0, v)) / 100) * 13;
        return (
          <rect
            // biome-ignore lint/suspicious/noArrayIndexKey: slots of a fixed ring
            key={i}
            x={3 + (offset + i) * 3}
            y={14.5 - h}
            width={2}
            height={h}
            fill="currentColor"
          />
        );
      })}
    </svg>
  );
}

/** Memory fill gauge, 6 x 16. */
function GaugeIcon({ pct }: { pct: number | null }) {
  const h =
    pct == null || !Number.isFinite(pct)
      ? 0
      : (Math.min(100, Math.max(0, pct)) / 100) * 13;
  return (
    <svg
      width="6"
      height="16"
      viewBox="0 0 6 16"
      aria-hidden
      className="shrink-0"
    >
      <rect
        x={0.5}
        y={0.5}
        width={5}
        height={15}
        rx={1.5}
        fill="none"
        stroke="currentColor"
        opacity={0.45}
      />
      {h > 0 && (
        <rect
          x={1.5}
          y={14.5 - h}
          width={3}
          height={h}
          rx={0.5}
          fill="currentColor"
        />
      )}
    </svg>
  );
}

/** Per-core strip: 2 pt bars on a 3 pt pitch, 3 pt more between core kinds. */
function CoresIcon({
  clusters,
}: {
  clusters: readonly (readonly (number | null)[])[];
}) {
  const bars: { x: number; v: number | null }[] = [];
  let x = 0;
  clusters.forEach((cluster, c) => {
    if (c > 0) x += 3;
    for (const v of cluster) {
      bars.push({ x, v });
      x += 3;
    }
  });
  const width = Math.max(1, x - 1);
  return (
    <svg width={width} height="16" viewBox={`0 0 ${width} 16`} aria-hidden>
      {bars.map(({ x, v }) => {
        const h =
          v == null
            ? 0
            : Math.max(1, (Math.min(100, Math.max(0, v)) / 100) * 16);
        return (
          <Fragment key={x}>
            <rect
              x={x}
              y={0}
              width={2}
              height={16}
              fill="currentColor"
              opacity={0.25}
            />
            {h > 0 && (
              <rect x={x} y={16 - h} width={2} height={h} fill="currentColor" />
            )}
          </Fragment>
        );
      })}
    </svg>
  );
}

function OwnItem({ item }: { item: TrayOwnItem }) {
  switch (item.kind) {
    case "rates":
      return <Rates up={item.up} down={item.down} />;
    case "value":
      return (
        <span className="inline-flex shrink-0 items-center gap-1">
          <Stacked text={item.label} />
          <Value>{item.text}</Value>
        </span>
      );
    case "gauge":
      return (
        <span className="inline-flex shrink-0 items-center gap-1">
          <Stacked text={item.label} />
          <GaugeIcon pct={item.pct} />
          <Value>{item.text}</Value>
        </span>
      );
    case "cores":
      return (
        <span className="inline-flex shrink-0 items-center gap-1">
          <Stacked text={item.label} />
          <CoresIcon clusters={item.clusters} />
        </span>
      );
    default:
      return (
        <span className="inline-flex shrink-0 items-center gap-1">
          <Stacked text={item.label} />
          {item.kind === "spark" ? (
            <SparkIcon samples={item.samples} />
          ) : (
            <HistIcon samples={item.samples} />
          )}
        </span>
      );
  }
}

function describe(layout: TrayLayout): string {
  const parts: string[] = layout.items.map((i) =>
    "text" in i ? `${i.label} ${i.text}` : `${i.module} item`
  );
  if (layout.combined) {
    const n = layout.combined.bars.length;
    if (n > 0) parts.push(`${n} ${n === 1 ? "bar" : "bars"}`);
    for (const r of layout.combined.readouts) {
      parts.push(
        r.kind === "rates"
          ? `network ${r.up}, ${r.down}`
          : `${r.readout} ${r.text}`
      );
    }
  }
  return `Menu bar preview: ${parts.join(", ") || "empty"}`;
}

/**
 * Simulated menu bar strip: the own items, then the combined item, then the
 * clock, as macOS places new status items. The real items are drawn in Rust
 * as template images; this mirrors their geometry so the onboarding and
 * Settings choices preview truthfully.
 */
export function TrayPreview({
  layout,
  clock = "10:40",
  className,
}: TrayPreviewProps) {
  const { combined } = layout;
  return (
    <div
      role="img"
      aria-label={describe(layout)}
      className={cn(
        "justify-end-safe flex h-[22px] items-center gap-3.5 overflow-x-auto rounded-control border border-border-subtle bg-deep px-2.5 text-foreground",
        className
      )}
    >
      {layout.items.map((item) => (
        <OwnItem key={item.module} item={item} />
      ))}
      {combined && (
        <span className="inline-flex shrink-0 items-center gap-2.5">
          {combined.bars.length > 0 && (
            <span className="inline-flex shrink-0 items-center gap-1">
              <BarsIcon bars={combined.bars} />
              {combined.readouts[0]?.kind === "marked" &&
                combined.readouts[0].marker.kind === "none" && (
                  <Value>{combined.readouts[0].text}</Value>
                )}
            </span>
          )}
          {combined.readouts.map((r, i) =>
            // "61°" right after the bars sits with them, 4 pt away.
            i === 0 &&
            combined.bars.length > 0 &&
            r.kind === "marked" &&
            r.marker.kind === "none" ? null : (
              <Readout key={r.readout} r={r} />
            )
          )}
        </span>
      )}
      <span className="shrink-0 text-[11px] opacity-60">{clock}</span>
    </div>
  );
}
