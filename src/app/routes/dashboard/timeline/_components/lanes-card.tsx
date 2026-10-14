import { eventLabel } from "@core/events";
import { formatClockSeconds, formatPercent } from "@core/format";
import type { Event } from "@core/generated/bindings";
import {
  type KeyboardEvent,
  type PointerEvent,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useElementSize } from "~/hooks/use-element-size";
import { cn } from "~/lib/utils";
import { Card } from "~/widgets/card";
import { type LaneData, useProcessesAt } from "../_hooks/use-timeline-data";
import { meanOf, peakOf } from "../_lib/buckets";
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
import { LaneCrosshair } from "./lane-crosshair";
import { LaneLabel } from "./lane-label";
import { LaneOverlays } from "./lane-overlays";
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
  const [plotColRef, width] = useElementSize("width");
  const sliderRef = useRef<HTMLDivElement>(null);
  const [cursor, setCursor] = useState<number | null>(null);
  const [tableLane, setTableLane] = useState<LaneData | null>(null);

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
    let next: number | null = null;
    switch (e.key) {
      case "ArrowLeft":
        next = at - step;
        break;
      case "ArrowRight":
        next = at + step;
        break;
      case "Home":
        next = fromMs;
        break;
      case "End":
        next = lastBucket;
        break;
    }
    if (next === null) return;
    e.preventDefault();
    moveCursor(Math.floor(next / bucketMs) * bucketMs);
  };

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

          <LaneOverlays
            ticks={ticks}
            bands={bands}
            lanes={lanes}
            shown={shown}
            width={width}
            fromMs={fromMs}
            toMs={toMs}
          />

          <LaneCrosshair
            lanes={lanes}
            bands={bands}
            models={models}
            cursor={cursor}
            width={width}
            fromMs={fromMs}
            toMs={toMs}
            bucketMs={bucketMs}
            units={units}
            resolution={resolution}
            processes={processes}
          />

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
              className="figures absolute top-1.5 -translate-x-1/2 text-[10px] text-fg-faint"
              style={{ left: `${fractionAt(t.tMs, fromMs, toMs) * 100}%` }}
            >
              {t.label}
            </span>
          ))}
          <span className="figures absolute top-1.5 right-0 text-[10px] text-muted-foreground">
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
