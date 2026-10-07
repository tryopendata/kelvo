import { cn } from "~/lib/utils";

export interface GapBandProps {
  fromMs: number;
  toMs: number;
  /** "Asleep 11:02–11:31 · not interpolated", "Paused", "Kelvo not running". */
  label: string;
  /**
   * The chart's x range. When given, the band positions itself absolutely
   * inside a `relative` plot box; without it, it fills its parent.
   */
  rangeFromMs?: number;
  rangeToMs?: number;
  /** Where the label sits: straddling the bottom edge, or hidden. */
  labelPosition?: "bottom" | "none";
}

/**
 * Hatched band over a span with no data: 1 px stripes at
 * 7 px in `--color-grid`, dashed edges, and a label saying why. Lines stop at
 * its edges; nothing is drawn across it.
 */
export function GapBand({
  fromMs,
  toMs,
  label,
  rangeFromMs,
  rangeToMs,
  labelPosition = "bottom",
}: GapBandProps) {
  let left: string | undefined;
  let width: string | undefined;
  if (rangeFromMs !== undefined && rangeToMs !== undefined) {
    const span = Math.max(1, rangeToMs - rangeFromMs);
    const a = Math.max(0, (fromMs - rangeFromMs) / span);
    const b = Math.min(1, (toMs - rangeFromMs) / span);
    left = `${(a * 100).toFixed(2)}%`;
    width = `${(Math.max(0, b - a) * 100).toFixed(2)}%`;
  }
  const positioned = left !== undefined;
  return (
    <div
      role="note"
      aria-label={label}
      data-gap-from={fromMs}
      data-gap-to={toMs}
      className={cn(
        "flex items-end justify-center border-border-strong border-x border-dashed bg-[repeating-linear-gradient(135deg,var(--color-grid)_0_1px,transparent_1px_7px)]",
        positioned ? "absolute inset-y-0" : "relative h-full min-h-12 w-full"
      )}
      style={positioned ? { left, width } : undefined}
    >
      {labelPosition === "bottom" && (
        <span className="translate-y-1/2 whitespace-nowrap rounded-control border border-border bg-card px-2 py-1 text-[11px] text-fg-subtle">
          {label}
        </span>
      )}
    </div>
  );
}

/** A labelled span with no data, in ms epoch; JSON-serializable. */
export interface GapSpan {
  fromMs: number;
  toMs: number;
  label: string;
}

/**
 * The gaps that overlap a chart's x range, as positioned bands. Render it
 * inside the chart's `relative` plot box; a band that runs past either edge
 * is clipped to it, and its label centres on the visible part.
 */
export function GapBands({
  gaps,
  rangeFromMs,
  rangeToMs,
}: {
  gaps: readonly GapSpan[];
  rangeFromMs: number;
  rangeToMs: number;
}) {
  if (rangeToMs <= rangeFromMs) return null;
  return gaps
    .filter((g) => g.toMs > rangeFromMs && g.fromMs < rangeToMs)
    .map((g) => (
      <GapBand
        key={`${g.fromMs}:${g.toMs}:${g.label}`}
        fromMs={g.fromMs}
        toMs={g.toMs}
        label={g.label}
        rangeFromMs={rangeFromMs}
        rangeToMs={rangeToMs}
      />
    ));
}
