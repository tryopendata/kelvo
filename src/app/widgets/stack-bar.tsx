import { Legend, type SwatchStep } from "./legend";
import { type Accent, accentVars, clamp01, rampBackground } from "./lib/accent";

export interface StackBarSegment {
  key: string;
  label: string;
  /** Formatted value for the legend and the accessible name ("3.9 GB"). */
  value: string;
  /**
   * Share of the whole bar. Segments are laid out in order. `null` is a
   * missing measurement: the segment is left out of the bar (its legend row
   * shows the missing value), and a bar with nothing measured is an empty
   * track.
   */
  fraction: number | null;
  /** `track` draws the remainder (free memory, rest of system power). */
  step: SwatchStep;
}

export interface StackBarProps {
  segments: StackBarSegment[];
  /** Defaults to the accent in scope (the card's `--a`). */
  accent?: Accent;
  showLegend?: boolean;
  legendColumns?: "inline" | 1 | 2;
  /** Extra legend rows with no segment (popover memory "Swap"). */
  extraLegend?: { label: string; value: string }[];
  /** Overrides the generated accessible name. */
  ariaLabel?: string;
}

/**
 * Composition bar, 10 px with 1 px gaps (memory and power). A
 * zero-width segment keeps its legend row so a hatched series that is often
 * zero (ANE, compressed) still has its key.
 */
export function StackBar({
  segments,
  accent,
  showLegend = false,
  legendColumns = 2,
  extraLegend = [],
  ariaLabel,
}: StackBarProps) {
  const label =
    ariaLabel ?? segments.map((s) => `${s.label} ${s.value}`).join(", ");
  return (
    <div
      className="flex flex-col gap-2"
      style={accent ? accentVars(accent) : undefined}
    >
      <div
        role="img"
        aria-label={label}
        className="flex h-2.5 gap-px overflow-hidden rounded-mark"
      >
        {segments.every((s) => s.fraction === null) && (
          <span data-missing className="flex-1 bg-track" />
        )}
        {segments.map((s) => {
          if (s.fraction === null) return null;
          const f = clamp01(s.fraction);
          if (f === 0) return null;
          return (
            <span
              key={s.key}
              className={s.step === "track" ? "bg-track" : undefined}
              style={{
                flexGrow: f,
                flexBasis: 0,
                background:
                  s.step === "track" || s.step === "none"
                    ? undefined
                    : rampBackground(s.step),
              }}
            />
          );
        })}
      </div>
      {showLegend && (
        <Legend
          columns={legendColumns}
          items={[
            ...segments.map((s) => ({
              label: s.label,
              value: s.value,
              step: s.step,
            })),
            ...extraLegend.map((e) => ({ ...e, step: "none" as const })),
          ]}
        />
      )}
    </div>
  );
}
