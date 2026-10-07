import { cn } from "~/lib/utils";
import { type Accent, accentVars, rampColor } from "./lib/accent";
import { useTickScroll } from "./lib/use-tick-scroll";

export interface MirrorBarsProps {
  /** Upload (or read) per interval, drawn above the baseline. `null` is a gap. */
  up: (number | null)[];
  /** Download (or write), drawn below. Same length as `up`. */
  down: (number | null)[];
  intervalMs: number;
  /** Timestamp of the last bar; when it advances by one interval the bars slide. */
  tEndMs?: number;
  accent: Accent;
  ariaLabel: string;
  /** Scale ceilings per side. Default: the largest value on that side. */
  upMax?: number;
  downMax?: number;
  /** Heights in px of the two halves (18 above, 30 below). */
  upHeight?: number;
  downHeight?: number;
  /**
   * Bar slots `[from, to)` that are selected; the others dim. Absent or
   * null: nothing is selected and every bar draws at full strength.
   */
  highlight?: { from: number; to: number } | null;
}

function heights(values: (number | null)[], max: number, h: number) {
  return values.map((v) =>
    v == null || !Number.isFinite(v) || max <= 0
      ? null
      : Math.min(1, Math.max(0, v / max)) * h
  );
}

function maxOf(values: (number | null)[]): number {
  let m = 0;
  for (const v of values) if (v != null && Number.isFinite(v) && v > m) m = v;
  return m;
}

/**
 * Mirrored bars: up above a 1 px axis in the accent at ramp
 * step 2, down below in step 1. A missing sample leaves an empty slot, never a
 * zero-height bar that reads as "idle". Direction is in the aria-label and in
 * the surrounding stat labels (↑ UPLOAD, ↓ DOWNLOAD), never color alone.
 */
export function MirrorBars({
  up,
  down,
  intervalMs,
  tEndMs,
  accent,
  ariaLabel,
  upMax,
  downMax,
  upHeight = 18,
  downHeight = 30,
  highlight,
}: MirrorBarsProps) {
  const n = Math.max(up.length, down.length, 1);
  const upH = heights(up, upMax ?? maxOf(up), upHeight);
  const downH = heights(down, downMax ?? maxOf(down), downHeight);
  const shift = `${(100 / n).toFixed(3)}%`;
  const upRef = useTickScroll<HTMLDivElement>(tEndMs, intervalMs, shift);
  const downRef = useTickScroll<HTMLDivElement>(tEndMs, intervalMs, shift);
  const slots = Array.from({ length: n }, (_, i) => i);
  const dim = (i: number) =>
    highlight != null && (i < highlight.from || i >= highlight.to);
  // Slot keys are positions in the window, not data identity: the window
  // scrolls by a transform, so the same slot index is reused each tick.
  return (
    <div
      role="img"
      aria-label={ariaLabel}
      className="flex flex-col overflow-hidden"
      style={accentVars(accent)}
    >
      <div
        ref={upRef}
        className="flex items-end gap-0.5"
        style={{ height: upHeight }}
      >
        {slots.map((i) => (
          <span
            key={i}
            data-gap={upH[i] == null || undefined}
            className={cn("flex-1 rounded-t-hair", dim(i) && "opacity-35")}
            style={{
              height: upH[i] ?? 0,
              background: rampColor(2),
            }}
          />
        ))}
      </div>
      <div className="h-px bg-axis" />
      <div
        ref={downRef}
        className="flex items-start gap-0.5"
        style={{ height: downHeight }}
      >
        {slots.map((i) => (
          <span
            key={i}
            data-gap={downH[i] == null || undefined}
            className={cn("flex-1 rounded-b-hair", dim(i) && "opacity-35")}
            style={{ height: downH[i] ?? 0, background: "var(--a)" }}
          />
        ))}
      </div>
    </div>
  );
}
