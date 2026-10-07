import { eventLabel } from "@core/events";
import {
  formatClockSeconds,
  formatPercent,
  type RateUnit,
  rateParts,
} from "@core/format";
import type { Event } from "@core/generated/bindings";
import {
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { cn } from "~/lib/utils";
import { Card } from "~/widgets/card";
import { GapBand } from "~/widgets/gap-band";
import { type LaneData, useProcessesAt } from "../_hooks/use-timeline-data";
import { bucketAt, meanOf, peakOf } from "../_lib/buckets";
import { formatMetric, type LaneUnits } from "../_lib/format";
import {
  type Band,
  type Marker,
  sleepMarkers,
  timelineBands,
} from "../_lib/gaps";
import { buildLaneModel } from "../_lib/lane-model";
import { LANE_GAP, LANE_HEIGHT, type LaneDef } from "../_lib/lanes";
import {
  axisTicks,
  dayLabel,
  endLabel,
  fractionAt,
  momentLabel,
  type Span,
} from "../_lib/time";
import { AnnotationRow } from "./annotation-row";
import { CrosshairTooltip, type TooltipRow } from "./crosshair-tooltip";
import { LaneLabel } from "./lane-label";
import { LanePlot } from "./lane-plot";
import { LaneTableDialog } from "./lane-table-dialog";

export interface LanesCardProps {
  lanes: LaneData[];
  gaps: Parameters<typeof timelineBands>[0];
  /** Detector and alert events (D-083); those outside the range are skipped. */
  events: readonly Event[];
  fromMs: number;
  toMs: number;
  bucketMs: number;
  resolution: string;
  span: Span;
  live: boolean;
  units: LaneUnits;
  /** "24 GB": installed memory, for the Memory lane's sub line. */
  memTotal: string | null;
  /** The plot column's width in px, whenever it is measured or resized. */
  onPlotWidth?: (px: number) => void;
}

const SPAN_ARIA: Record<Span, string> = {
  "1h": "last hour",
  "6h": "6 hours",
  "24h": "last 24 hours",
  "7d": "last 7 days",
  "30d": "last 30 days",
};

const TOOLTIP_W = 276;
/** Narrowest gap band, in px, that gets hollow dots at its edges. */
const EDGE_DOT_MIN_PX = 16;

/** Lanes a band covers: a whole-host gap covers all, a module gap its lane. */
const covers = (band: Band, def: LaneDef) =>
  band.module === null || band.module === def.module;

function subLine(
  lane: LaneData,
  units: LaneUnits,
  memTotal: string | null,
  span: Span
): string {
  const s = lane.series;
  switch (lane.def.id) {
    case "cpu": {
      const peak = peakOf(s["cpu.total"] ?? []);
      return peak
        ? `peak ${formatPercent(peak.value)} · ${momentLabel(span, peak.tMs)}`
        : "no samples in range";
    }
    case "gpu": {
      const util = meanOf(s["gpu.util"] ?? []);
      const freq = meanOf(s["gpu.freq"] ?? []);
      return `avg ${formatPercent(util)} · ${formatMetric("gpu.freq", freq, units)}`;
    }
    case "memory":
      return memTotal ? `pressure · ${memTotal}` : "pressure";
    case "power":
      return "CPU solid · rest faint";
    case "temp":
      return "hottest SoC zone";
    default:
      return "↑ above · ↓ below";
  }
}

/** "8.2 / 0.4 MB/s": down then up, in the down figure's unit. */
function downUp(
  down: number | null,
  up: number | null,
  units: LaneUnits
): string {
  if (down === null || up === null) {
    return `${formatMetric("net.rx_total", down, units)} / ${formatMetric("net.tx_total", up, units)}`;
  }
  const d = rateParts(down, { units: units.rate });
  const u = rateParts(up, {
    units: units.rate,
    unit: d.unit as RateUnit,
  });
  return `${d.value} / ${u.value} ${d.unit}`;
}

/**
 * The stacked lanes (top region): label column, one uPlot per lane
 * on a shared x axis, gap bands, the Sleep/Wake row, and one crosshair that
 * follows the pointer (or arrow keys) across every lane with a dot per series
 * and a single tooltip.
 */
export function LanesCard({
  lanes,
  gaps,
  events,
  fromMs,
  toMs,
  bucketMs,
  resolution,
  span,
  live,
  units,
  memTotal,
  onPlotWidth,
}: LanesCardProps) {
  const plotColRef = useRef<HTMLDivElement>(null);
  const sliderRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [cursor, setCursor] = useState<number | null>(null);
  const [tableLane, setTableLane] = useState<LaneData | null>(null);

  useEffect(() => {
    const el = plotColRef.current;
    if (!el) return;
    setWidth(el.clientWidth);
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(([entry]) => {
      if (entry) setWidth(entry.contentRect.width);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    if (width > 0) onPlotWidth?.(width);
  }, [width, onPlotWidth]);

  const bands = useMemo(
    () => timelineBands(gaps, fromMs, toMs, span),
    [gaps, fromMs, toMs, span]
  );
  // A gap under EDGE_DOT_MIN_PX wide gets no hollow edge dots, in whole
  // buckets so resizing re-models only when the cut moves by one.
  const minEdgeGapMs =
    width > 0
      ? Math.ceil(((toMs - fromMs) * EDGE_DOT_MIN_PX) / width / bucketMs) *
        bucketMs
      : 0;
  const models = useMemo(
    () =>
      lanes.map((l) =>
        buildLaneModel(
          l.def,
          l.series,
          l.holds,
          bucketMs,
          bands.filter((b) => covers(b, l.def)),
          minEdgeGapMs
        )
      ),
    [lanes, bucketMs, bands, minEdgeGapMs]
  );
  // Live, the open bucket's events show too (pulled inside the right edge),
  // so a pushed event appears at once rather than when its bucket closes.
  const shown = useMemo(() => {
    const end = live ? toMs + bucketMs : toMs;
    return events.filter((e) => e.ts_ms >= fromMs && e.ts_ms < end);
  }, [events, fromMs, toMs, bucketMs, live]);
  // 30d holds about 60 Sleep and Wake markers; at a few pixels per night
  // they only merge into "+N" pills. The bands mark the nights there.
  const markers = useMemo(() => {
    const out: Marker[] = shown.map((e) => ({
      tMs: e.ts_ms,
      kind: "event",
      label: eventLabel(e),
    }));
    if (span !== "30d") out.push(...sleepMarkers(gaps, fromMs, toMs));
    return out;
  }, [shown, gaps, fromMs, toMs, span]);
  const ticks = useMemo(
    () => axisTicks(fromMs, toMs, span),
    [fromMs, toMs, span]
  );
  const stackHeight =
    lanes.length * LANE_HEIGHT + Math.max(0, lanes.length - 1) * LANE_GAP;

  const processes = useProcessesAt(cursor, bucketMs);

  const lastBucket = toMs - bucketMs;
  const moveCursor = (t: number) =>
    setCursor(Math.min(lastBucket, Math.max(fromMs, t)));
  // A pill click puts the crosshair on the event and focus on the slider,
  // so the arrow keys step on from there.
  const selectEvent = (tMs: number) => {
    moveCursor(Math.floor(tMs / bucketMs) * bucketMs);
    sliderRef.current?.focus();
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    if (rect.width <= 0) return;
    const t = fromMs + ((e.clientX - rect.left) / rect.width) * (toMs - fromMs);
    moveCursor(Math.floor(t / bucketMs) * bucketMs);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step = e.shiftKey ? bucketMs * 10 : bucketMs;
    const at = cursor ?? lastBucket;
    const next =
      e.key === "ArrowLeft"
        ? at - step
        : e.key === "ArrowRight"
          ? at + step
          : e.key === "Home"
            ? fromMs
            : e.key === "End"
              ? lastBucket
              : null;
    if (next === null) return;
    e.preventDefault();
    moveCursor(Math.floor(next / bucketMs) * bucketMs);
  };

  let crosshair: ReactNode = null;
  if (cursor !== null && width > 0) {
    const frac = fractionAt(cursor + bucketMs / 2, fromMs, toMs);
    const x = frac * width;
    const gapHere = bands.find(
      (b) => b.module === null && cursor >= b.fromMs && cursor < b.toMs
    );
    const value = (lane: LaneData | undefined, metric: string) =>
      lane
        ? (bucketAt(lane.series[metric] ?? [], cursor, bucketMs)?.avg ?? null)
        : null;
    const byId = (id: LaneDef["id"]) => lanes.find((l) => l.def.id === id);
    const rows: TooltipRow[] = [];
    const add = (
      id: LaneDef["id"],
      label: string,
      metric: string,
      accent: TooltipRow["accent"]
    ) => {
      const lane = byId(id);
      if (lane) {
        rows.push({
          label,
          accent,
          value: formatMetric(metric, value(lane, metric), units),
        });
      }
    };
    add("cpu", "CPU", "cpu.total", "cpu");
    add("gpu", "GPU", "gpu.util", "gpu");
    add("memory", "Memory pressure", "mem.pressure", "mem");
    add("power", "Power (system)", "power.system", "power");
    add("temp", "Hottest SoC zone", "thermal.hottest", "temp");
    const temp = byId("temp");
    if (temp && (temp.series["fan.rpm"]?.length ?? 0) > 0) {
      add("temp", "Fans", "fan.rpm", "temp");
    }
    const net = byId("network");
    if (net) {
      rows.push({
        label: "Network ↓ / ↑",
        accent: "net",
        value: downUp(
          value(net, "net.rx_total"),
          value(net, "net.tx_total"),
          units
        ),
      });
    }

    const tipLeft =
      x + 14 + TOOLTIP_W <= width ? x + 14 : Math.max(0, x - 14 - TOOLTIP_W);
    crosshair = (
      <>
        <div
          aria-hidden
          className="pointer-events-none absolute -top-1.5 bottom-0 w-px bg-foreground/45"
          style={{ left: x }}
        />
        {models.flatMap((m, i) =>
          m.series.map((s) => {
            const v = value(lanes[i], s.metric);
            if (v === null) return null;
            const top =
              i * (LANE_HEIGHT + LANE_GAP) +
              m.yFraction(s.metric, v) * LANE_HEIGHT;
            return (
              <span
                key={`${m.def.id}-${s.metric}`}
                aria-hidden
                className="pointer-events-none absolute -mt-1 -ml-[3.5px] size-[7px] rounded-full border-[1.5px] bg-background"
                style={{
                  left: x,
                  top,
                  borderColor: `var(--color-${s.accent}-ink)`,
                }}
              />
            );
          })
        )}
        <CrosshairTooltip
          title={`${dayLabel(cursor)} · ${formatClockSeconds(cursor)}`}
          resolution={resolution}
          rows={rows}
          processes={gapHere ? null : processes}
          note={gapHere?.label}
          style={{ left: tipLeft, top: 2 }}
        />
      </>
    );
  }

  return (
    <Card
      accent="cpu"
      variant="chart"
      ariaLabel="Stacked lanes"
      className="flex flex-col gap-1.5 px-4 pt-4 pb-3"
    >
      <div className="grid grid-cols-[148px_minmax(0,1fr)] gap-x-4">
        <div />
        <AnnotationRow
          markers={markers}
          fromMs={fromMs}
          toMs={toMs}
          widthPx={width}
          onSelect={selectEvent}
        />
      </div>

      <div className="grid grid-cols-[148px_minmax(0,1fr)] gap-x-4">
        <div className="flex flex-col gap-3">
          {lanes.map((l) => (
            <LaneLabel
              key={l.def.id}
              def={l.def}
              sub={subLine(l, units, memTotal, span)}
              units={units}
              onShowTable={() => setTableLane(l)}
            />
          ))}
        </div>

        <div
          ref={plotColRef}
          className="relative"
          style={{ height: stackHeight }}
        >
          <div className="flex flex-col gap-3">
            {models.map((m) => (
              <LanePlot
                key={m.def.id}
                x={m.x}
                series={m.series}
                domain={m.domain}
                baseline={m.baseline}
                fromMs={fromMs}
                toMs={toMs}
                height={LANE_HEIGHT}
                ariaLabel={`${m.def.ariaLabel}, ${SPAN_ARIA[span]}`}
              />
            ))}
          </div>

          {ticks.map((t) => (
            <div
              key={t.tMs}
              aria-hidden
              className="pointer-events-none absolute inset-y-0 w-px bg-border-subtle"
              style={{ left: `${fractionAt(t.tMs, fromMs, toMs) * 100}%` }}
            />
          ))}

          {bands.map((b) => {
            const lane = lanes.findIndex((l) => l.def.module === b.module);
            if (b.module !== null && lane === -1) return null;
            const wide = (b.toMs - b.fromMs) / (toMs - fromMs) > 0.12;
            const mid = fractionAt((b.fromMs + b.toMs) / 2, fromMs, toMs);
            return (
              <div
                key={`${b.fromMs}-${b.module ?? "all"}`}
                className="pointer-events-none absolute inset-x-0"
                style={
                  b.module === null
                    ? { top: 0, bottom: 0 }
                    : {
                        top: lane * (LANE_HEIGHT + LANE_GAP),
                        height: LANE_HEIGHT,
                      }
                }
              >
                <GapBand
                  fromMs={b.fromMs}
                  toMs={b.toMs}
                  label={b.label}
                  rangeFromMs={fromMs}
                  rangeToMs={toMs}
                  labelPosition="none"
                />
                {wide && (
                  <span
                    aria-hidden
                    className="absolute top-1/2 -translate-x-1/2 -translate-y-1/2 whitespace-nowrap rounded-control border border-border bg-card px-2 py-[3px] font-normal text-[11px] text-muted-foreground"
                    style={{ left: `${mid * 100}%` }}
                  >
                    {b.label}
                  </span>
                )}
              </div>
            );
          })}

          {width > 0 &&
            shown.map((e) => {
              // The episode, at least 3 px wide so an instant change shows.
              const x0 = fractionAt(e.start_ms, fromMs, toMs) * width;
              const x1 = fractionAt(e.ts_ms, fromMs, toMs) * width;
              const w = Math.max(3, x1 - x0);
              return (
                <div
                  key={`${e.detail.kind}-${e.ts_ms}`}
                  aria-hidden
                  data-event-band={e.ts_ms}
                  className="pointer-events-none absolute inset-y-0 border-warning/20 border-x bg-warning/[0.07]"
                  style={{ left: Math.min(x0, x1 - w + 1), width: w }}
                />
              );
            })}

          {crosshair}

          <div
            ref={sliderRef}
            role="slider"
            tabIndex={0}
            aria-label="Timeline cursor"
            aria-valuemin={fromMs}
            aria-valuemax={lastBucket}
            aria-valuenow={cursor ?? lastBucket}
            aria-valuetext={
              cursor === null
                ? "No time selected"
                : `${dayLabel(cursor)} ${formatClockSeconds(cursor)}`
            }
            className={cn(
              "absolute inset-0 cursor-crosshair rounded-tile outline-none focus-visible:ring-2 focus-visible:ring-ring"
            )}
            onPointerMove={onPointerMove}
            onPointerLeave={() => setCursor(null)}
            onKeyDown={onKeyDown}
            onFocus={() => setCursor((c) => c ?? lastBucket)}
            onBlur={() => setCursor(null)}
          />
        </div>
      </div>

      <div className="grid grid-cols-[148px_minmax(0,1fr)] gap-x-4">
        <div />
        <div className="relative h-5">
          {ticks.map((t) => (
            <span
              key={t.tMs}
              className="data-mono absolute top-1.5 -translate-x-1/2 text-[10px] text-fg-faint"
              style={{ left: `${fractionAt(t.tMs, fromMs, toMs) * 100}%` }}
            >
              {t.label}
            </span>
          ))}
          <span className="data-mono absolute top-1.5 right-0 text-[10px] text-muted-foreground">
            {endLabel(span, toMs, live)}
          </span>
        </div>
      </div>

      <LaneTableDialog
        lane={tableLane}
        bands={bands}
        fromMs={fromMs}
        toMs={toMs}
        units={units}
        onClose={() => setTableLane(null)}
      />
    </Card>
  );
}
