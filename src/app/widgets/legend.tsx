import { cn } from "~/lib/utils";
import {
  type Accent,
  accentVars,
  type RampStep,
  rampBackground,
  rampColor,
} from "./lib/accent";

/** A ramp step, the empty track (a "free" remainder), or no swatch at all. */
export type SwatchStep = RampStep | "track" | "none";

export interface LegendItem {
  label: string;
  /** Absent: the label alone, as for a series whose figure sits elsewhere. */
  value?: string;
  step: SwatchStep;
  color?: string | undefined;
  /** Striped swatch for a dashed line (StreamArea `dash`). */
  dashed?: boolean;
  onFocus?: () => void;
  onBlur?: () => void;
  onClick?: () => void;
  pressed?: boolean;
}

export interface LegendProps {
  items: LegendItem[];
  /** Defaults to the accent in scope (the card's `--a`). */
  accent?: Accent;
  /**
   * `inline`: one wrapping row, value right after the label (Overview, 04).
   * 2: a two-column grid with values right-aligned (popover memory, 14).
   */
  columns?: "inline" | 1 | 2;
}

/**
 * The 8 px legend key: a ramp swatch, the track, or an empty slot. `dashed`
 * stripes a ramp step across, the key for a dashed line.
 */
export function Swatch({
  step,
  size = 8,
  color,
  dashed = false,
}: {
  step: SwatchStep;
  size?: 7 | 8;
  color?: string | undefined;
  dashed?: boolean | undefined;
}) {
  const dim = size === 8 ? "size-2" : "size-[7px]";
  if (step === "none" && !color)
    return <span aria-hidden className={cn(dim, "shrink-0")} />;
  return (
    <span
      aria-hidden
      data-dashed={dashed || undefined}
      className={cn(
        dim,
        "shrink-0 rounded-mark",
        step === "track" &&
          "bg-track shadow-[inset_0_0_0_1px_var(--color-border)]"
      )}
      style={
        color
          ? { background: color }
          : step === "track"
            ? undefined
            : { background: swatchBackground(step, dashed) }
      }
    />
  );
}

function swatchBackground(step: RampStep | "none", dashed: boolean): string {
  if (dashed && typeof step === "number") {
    return `repeating-linear-gradient(90deg, ${rampColor(step)} 0 3px, transparent 3px 5px)`;
  }
  return rampBackground(step === "none" ? 1 : step);
}

/** Swatch, label and value rows. */
export function Legend({ items, accent, columns = "inline" }: LegendProps) {
  const style = accent ? accentVars(accent) : undefined;

  if (columns === "inline") {
    return (
      <ul className="flex flex-wrap gap-3.5" style={style}>
        {items.map((item) => (
          <li
            key={item.label}
            className="inline-flex items-center gap-1.5 font-normal text-[11px] text-fg-subtle"
          >
            {item.onClick ? (
              <button
                type="button"
                aria-pressed={item.pressed}
                onPointerEnter={item.onFocus}
                onPointerLeave={item.onBlur}
                onFocus={item.onFocus}
                onBlur={item.onBlur}
                onClick={item.onClick}
                className="inline-flex items-center gap-1.5 rounded-control focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              >
                <Swatch
                  step={item.step}
                  color={item.color}
                  dashed={item.dashed}
                />
                {item.label}
                {item.value !== undefined && (
                  <span className="figures text-foreground">{item.value}</span>
                )}
              </button>
            ) : (
              <>
                <Swatch
                  step={item.step}
                  color={item.color}
                  dashed={item.dashed}
                />
                {item.label}
                {item.value !== undefined && (
                  <span className="figures text-foreground">{item.value}</span>
                )}
              </>
            )}
          </li>
        ))}
      </ul>
    );
  }

  return (
    <ul
      className={cn(
        "grid gap-x-4 gap-y-1 text-[11px]",
        columns === 2 ? "grid-cols-2" : "grid-cols-1"
      )}
      style={style}
    >
      {items.map((item) => (
        <li key={item.label} className="flex items-center gap-1.5">
          <Swatch step={item.step} color={item.color} dashed={item.dashed} />
          <span className="flex-1 font-normal text-fg-subtle">
            {item.label}
          </span>
          {item.value !== undefined && (
            <span className="figures">{item.value}</span>
          )}
        </li>
      ))}
    </ul>
  );
}
