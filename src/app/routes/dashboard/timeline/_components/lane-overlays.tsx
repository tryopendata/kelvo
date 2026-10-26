import type { Event } from "@core/generated/bindings";
import { Pause } from "lucide-react";
import { cn } from "~/lib/utils";
import { GapBand } from "~/widgets/gap-band";
import type { LaneData } from "../_hooks/use-timeline-data";
import type { Band } from "../_lib/gaps";
import { LANE_GAP, LANE_HEIGHT } from "../_lib/lanes";
import type { axisTicks } from "../_lib/time";
import { fractionAt } from "../_lib/time";
export function LaneOverlays({
  ticks,
  bands,
  lanes,
  shown,
  width,
  fromMs,
  toMs,
}: {
  ticks: ReturnType<typeof axisTicks>;
  bands: Band[];
  lanes: LaneData[];
  shown: Event[];
  width: number;
  fromMs: number;
  toMs: number;
}) {
  return (
    <>
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
            className={cn(
              "pointer-events-none absolute inset-x-0",
              b.module === null && "inset-y-0"
            )}
            style={
              b.module === null
                ? undefined
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
              reason={b.reason}
              rangeFromMs={fromMs}
              rangeToMs={toMs}
              labelPosition="none"
            />
            {wide && b.showCaption && (
              <span
                aria-hidden
                className={cn(
                  "absolute top-1/2 inline-flex -translate-x-1/2 -translate-y-1/2 items-center gap-1 whitespace-nowrap font-normal text-[11px] text-muted-foreground",
                  b.reason !== "paused" &&
                    "rounded-control border border-border bg-card px-2 py-[3px]"
                )}
                style={{ left: `${mid * 100}%` }}
              >
                {b.reason === "paused" && <Pause className="size-3" />}
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
    </>
  );
}
