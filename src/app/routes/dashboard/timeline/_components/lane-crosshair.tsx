import { formatClockSeconds } from "@core/format";
import type { ProcessesAt } from "@core/generated/bindings";
import type { LaneData } from "../_hooks/use-timeline-data";
import type { LaneUnits } from "../_lib/format";
import type { Band } from "../_lib/gaps";
import type { buildLaneModel } from "../_lib/lane-model";
import { LANE_GAP, LANE_HEIGHT } from "../_lib/lanes";
import { dayLabel, fractionAt } from "../_lib/time";
import { CrosshairTooltip } from "./crosshair-tooltip";
import { laneTooltipRows, laneValue } from "./lane-tooltip-rows";

const TOOLTIP_W = 276;
export function LaneCrosshair({
  lanes,
  bands,
  models,
  cursor,
  width,
  fromMs,
  toMs,
  bucketMs,
  units,
  resolution,
  processes,
}: {
  lanes: LaneData[];
  bands: Band[];
  models: ReturnType<typeof buildLaneModel>[];
  cursor: number | null;
  width: number;
  fromMs: number;
  toMs: number;
  bucketMs: number;
  units: LaneUnits;
  resolution: string;
  processes: ProcessesAt | null | undefined;
}) {
  if (cursor === null || width <= 0) return null;
  const frac = fractionAt(cursor + bucketMs / 2, fromMs, toMs);
  const x = frac * width;
  const gapHere = bands.find(
    (b) => b.module === null && cursor >= b.fromMs && cursor < b.toMs
  );
  const value = (lane: LaneData | undefined, metric: string) =>
    laneValue(lane, metric, cursor, bucketMs);
  const rows = laneTooltipRows(lanes, cursor, bucketMs, units);
  const tipLeft =
    x + 14 + TOOLTIP_W <= width ? x + 14 : Math.max(0, x - 14 - TOOLTIP_W);
  return (
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
        style={{ left: tipLeft }}
      />
    </>
  );
}
