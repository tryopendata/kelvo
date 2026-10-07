import { brushBucketMs, floorTo, rangeSlots } from "@core/brush";
import { gridIntervalMs } from "@core/live-state";
import { windowWords } from "@core/live-window";
import { useRef } from "react";
import { BrushOverlay } from "~/components/brush-overlay";
import { MirrorHover } from "~/components/mirror-hover";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useRingBuckets } from "~/hooks/use-ring";
import { useBrush } from "~/stores/brush-store";
import { useHost } from "~/stores/host-store";
import { GapBands, type GapSpan } from "~/widgets/gap-band";
import type { Accent } from "~/widgets/lib/accent";
import { windowTicks } from "~/widgets/lib/chart-labels";
import { FIELD_LABEL } from "~/widgets/lib/classes";
import { MirrorBars } from "~/widgets/mirror-bars";

/** Bars across the chart: one per interval up to this, then wider buckets. */
const MAX_BARS = 120;

/**
 * Bucket width and count for a mirrored chart over `windowMs`: one bar per
 * sample for short windows, at most `MAX_BARS` bars otherwise, each the
 * average of its bucket.
 */
export function mirrorBuckets(
  windowMs: number,
  intervalMs: number
): { bucketMs: number; count: number } {
  const samples = Math.max(1, Math.round(windowMs / intervalMs));
  const per = Math.max(1, Math.ceil(samples / MAX_BARS));
  const bucketMs = per * intervalMs;
  return { bucketMs, count: Math.max(1, Math.round(windowMs / bucketMs)) };
}

export interface LiveMirrorChartProps {
  /**
   * Series drawn above the axis (upload, read). Pages pass a Rust total
   * (`net.tx_total`, D-092), so the chart never adds interfaces itself.
   */
  upKey: string;
  /** Series drawn below the axis (download, write). */
  downKey: string;
  /** "Upload", "Read". */
  upLabel: string;
  downLabel: string;
  windowMs: number;
  accent: Accent;
  /** Formats a ceiling for the side labels ("40 MB/s"). */
  format: (v: number) => string;
  /** Smallest ceiling either side autoscales to, in the series' unit. */
  minCeiling: number;
  upHeight?: number;
  downHeight?: number;
  /** Labelled gaps drawn as hatched bands over the bars. */
  gaps?: readonly GapSpan[];
  /**
   * Time-range selection (D-089): needs a `BrushProvider` above.
   * Bars then use widths that divide 10 s (5 s at 5m, 10 s at 15m), so a
   * selection snapped to 10 s buckets lines up with them.
   */
  brush?: boolean;
}

/**
 * Module-page mirrored chart (plan 4.11, 4.12): `MirrorBars` from the
 * popover at page size, fed from the live ring. Each side autoscales on its
 * own with the 60 s shrink hysteresis and states its ceiling, so a small
 * upload next to a large download stays readable.
 */
export function LiveMirrorChart({
  upKey,
  downKey,
  upLabel,
  downLabel,
  windowMs,
  accent,
  format,
  minCeiling,
  upHeight = 72,
  downHeight = 120,
  gaps,
  brush = false,
}: LiveMirrorChartProps) {
  const intervalMs = useHost((s) => gridIntervalMs(s.status));
  const plain = mirrorBuckets(windowMs, intervalMs);
  const bucketMs = brush
    ? brushBucketMs(plain.bucketMs, intervalMs)
    : plain.bucketMs;
  const count = Math.max(1, Math.round(windowMs / bucketMs));
  // The newest elapsed 10 s edge: the brush selects no time after it. Moves
  // every 10 s, not per tick.
  const elapsedEdge = useHost((s) =>
    brush && s.lastTsMs !== null ? floorTo(s.lastTsMs) : null
  );
  // Draft or committed; changes on a drag and a selection, not per pointer move.
  const selected = useBrush((s) => (brush ? (s.draft ?? s.range) : null));
  const { values, endMs } = useRingBuckets([upKey, downKey], bucketMs, count);
  const pick = (key: string) =>
    Array.from({ length: count }, (_, i) => values[key]?.[i] ?? null);
  const up = pick(upKey);
  const down = pick(downKey);
  const now = endMs ?? 0;
  const upMax = useNiceCeiling(up, now, minCeiling, windowMs);
  const downMax = useNiceCeiling(down, now, minCeiling, windowMs);
  const ticks = windowTicks(windowMs, 5);
  const firstMs = now - (count - 1) * bucketMs;
  const slots =
    selected && endMs !== null
      ? rangeSlots(selected, firstMs, bucketMs, count)
      : null;
  // A selection scrolled off the chart dims nothing.
  const highlight = slots && slots.to > slots.from ? slots : null;
  const plotRef = useRef<HTMLDivElement>(null);

  return (
    <div className="flex flex-col gap-1.5">
      <div className="relative pl-24">
        <div
          aria-hidden
          className="absolute top-0 left-0 flex w-22 flex-col justify-between"
          style={{ height: upHeight + downHeight + 1 }}
        >
          <span className={FIELD_LABEL}>
            ↑ {upLabel}
            <span className="block normal-case tracking-normal">
              {format(upMax)}
            </span>
          </span>
          <span className={FIELD_LABEL}>
            <span className="block normal-case tracking-normal">
              {format(downMax)}
            </span>
            ↓ {downLabel}
          </span>
        </div>
        <div ref={plotRef}>
          <MirrorBars
            up={up}
            down={down}
            intervalMs={bucketMs}
            tEndMs={endMs ?? undefined}
            upMax={upMax}
            downMax={downMax}
            upHeight={upHeight}
            downHeight={downHeight}
            accent={accent}
            highlight={highlight}
            ariaLabel={`${upLabel} above the line, ${downLabel.toLowerCase()} below, last ${windowWords(windowMs)}${brush ? ". Drag to select a range." : ""}`}
          />
        </div>
        {brush && endMs !== null && (
          <BrushOverlay
            firstMs={firstMs}
            spanMs={count * bucketMs}
            selectableToMs={elapsedEdge ?? undefined}
            height={upHeight + downHeight + 1}
          />
        )}
        {gaps && gaps.length > 0 && endMs !== null && (
          <div
            className="pointer-events-none absolute top-0 right-0 left-24"
            style={{ height: upHeight + downHeight + 1 }}
          >
            <GapBands
              gaps={gaps}
              rangeFromMs={firstMs}
              rangeToMs={endMs + bucketMs}
            />
          </div>
        )}
        {endMs !== null && (
          <MirrorHover
            plotRef={plotRef}
            inset={96}
            height={upHeight + downHeight + 1}
            count={count}
            firstMs={firstMs}
            bucketMs={bucketMs}
            up={up}
            down={down}
            upLabel={upLabel}
            downLabel={downLabel}
            gaps={gaps}
            format={format}
            accent={accent}
            column={!brush}
          />
        )}
      </div>
      <div
        aria-hidden
        className="data-mono flex justify-between pl-24 text-[10px] text-fg-faint"
      >
        {ticks.map((t, i) => (
          <span
            key={t}
            className={i === ticks.length - 1 ? "text-muted-foreground" : ""}
          >
            {t}
          </span>
        ))}
      </div>
    </div>
  );
}
