import type { TimeRange } from "@core/brush";
import { formatClockSeconds } from "@core/format";
import type { Event, SeriesStats } from "@core/generated/bindings";
import { useEffect, useMemo, useState } from "react";
import { useElementSize } from "~/hooks/use-element-size";
import { cn } from "~/lib/utils";
import { useHostId } from "~/stores/host-store";
import { Card } from "~/widgets/card";
import { type LaneData, useProcessesAt } from "../_hooks/use-timeline-data";
import { useTimelineSelection } from "../_hooks/use-timeline-selection";
import { useZoomPreview } from "../_hooks/use-zoom-preview";
import { eventRange } from "../_lib/events";
import type { LaneUnits } from "../_lib/format";
import {
  type Band,
  type Marker,
  sleepMarkers,
  timelineBands,
} from "../_lib/gaps";
import { buildLaneModel } from "../_lib/lane-model";
import { LANE_GAP, LANE_HEIGHT, type LaneDef } from "../_lib/lanes";
import { laneSummary } from "../_lib/summaries";
import {
  axisTicks,
  dayLabel,
  endLabel,
  fractionAt,
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
  summaries?: SeriesStats | undefined;
  summaryError?: boolean;
  /** The plot column's width in px, whenever it is measured or resized. */
  onPlotWidth?: (px: number) => void;
  onSelectRange?: (range: TimeRange) => undefined | Promise<boolean>;
  selectionNowMs?: number;
  retainedFromMs?: number;
  loading?: boolean;
  failed?: boolean;
}

/** Narrowest gap band, in px, that gets hollow dots at its edges. */
const EDGE_DOT_MIN_PX = 16;

/** Lanes a band covers: a whole-host gap covers all, a module gap its lane. */
const covers = (band: Band, def: LaneDef) =>
  band.module === null || band.module === def.module;

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
  summaries,
  summaryError = false,
  onPlotWidth,
  onSelectRange,
  selectionNowMs = toMs,
  retainedFromMs = 0,
  loading = false,
  failed = false,
}: LanesCardProps) {
  const [plotColRef, width] = useElementSize("width");
  const hostId = useHostId();
  const zoom = useZoomPreview(
    plotColRef,
    { fromMs, toMs },
    resolution,
    loading,
    hostId,
    failed
  );
  const selectRange = (range: TimeRange) => {
    zoom.begin(range);
    void Promise.resolve(onSelectRange?.(range))
      .then((saved) => {
        if (saved === false) zoom.cancel();
      })
      .catch(zoom.cancel);
  };
  const {
    sliderRef,
    cursor,
    preview,
    selecting,
    sliderProps,
    selectEvent,
    focusAfterRange,
  } = useTimelineSelection({
    fromMs,
    toMs,
    bucketMs,
    width,
    nowMs: selectionNowMs,
    retainedFromMs,
    loading: loading || zoom.preview !== null,
    onSelectRange: onSelectRange ? selectRange : undefined,
  });
  const [activeEvents, setActiveEvents] = useState<readonly Event[]>([]);
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
    const out: Marker[] = [];
    if (span !== "30d") out.push(...sleepMarkers(gaps, fromMs, toMs));
    return out;
  }, [gaps, fromMs, toMs, span]);
  const ticks = useMemo(
    () => axisTicks(fromMs, toMs, span),
    [fromMs, toMs, span]
  );
  const stackHeight =
    lanes.length * LANE_HEIGHT + Math.max(0, lanes.length - 1) * LANE_GAP;

  const processes = useProcessesAt(selecting ? null : cursor, bucketMs);
  const lastBucket = Math.max(fromMs, toMs - bucketMs);

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
          events={shown}
          onActiveEvents={setActiveEvents}
          onSelectEvents={(members) => {
            const range = eventRange(
              members,
              bucketMs,
              selectionNowMs,
              retainedFromMs
            );
            if (
              !range ||
              !onSelectRange ||
              range.toMs - range.fromMs >= toMs - fromMs
            )
              return false;
            focusAfterRange.current = true;
            selectRange(range);
            return true;
          }}
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
              summary={laneSummary(l.def.id, summaries, units, summaryError)}
              onShowTable={() => setTableLane(l)}
            />
          ))}
        </div>

        <div
          ref={plotColRef}
          className="relative"
          style={{ height: stackHeight }}
        >
          <div data-lane-content className="absolute inset-0">
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
                  ariaLabel={`${m.def.ariaLabel}, recorded range from ${dayLabel(fromMs)} ${formatClockSeconds(fromMs)} to ${dayLabel(toMs)} ${formatClockSeconds(toMs)}`}
                />
              ))}
            </div>

            <LaneOverlays
              ticks={ticks}
              bands={bands}
              lanes={lanes}
              shown={activeEvents.filter(
                (e) => e.ts_ms >= fromMs && e.ts_ms < toMs
              )}
              width={width}
              fromMs={fromMs}
              toMs={toMs}
            />
          </div>
          <div
            ref={zoom.overlayRef}
            aria-hidden
            className="pointer-events-none absolute inset-0 overflow-hidden bg-card"
            style={{ visibility: zoom.preview ? "visible" : "hidden" }}
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

          {preview && (
            <div
              aria-hidden
              className="pointer-events-none absolute inset-y-0 border-primary border-x bg-primary/14"
              style={{
                left: `${fractionAt(preview.fromMs, fromMs, toMs) * 100}%`,
                width: `${(fractionAt(preview.toMs, fromMs, toMs) - fractionAt(preview.fromMs, fromMs, toMs)) * 100}%`,
              }}
            />
          )}
          <div
            ref={sliderRef}
            role="slider"
            tabIndex={0}
            aria-label="Timeline cursor"
            aria-description="Arrow keys inspect. Hold Shift and use arrows to select a range, then Enter to zoom. Escape cancels."
            aria-busy={loading}
            style={{ pointerEvents: zoom.preview ? "none" : undefined }}
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
            {...sliderProps}
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

      {zoom.preview && (
        <p role="status" className="text-[11px] text-muted-foreground">
          {failed ? "Recorded detail unavailable" : "Loading recorded detail"} ·
          preview at {zoom.preview.resolution}
        </p>
      )}
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
