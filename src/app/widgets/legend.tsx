import { cn } from "~/lib/utils";
import {
  type Accent,
  accentVars,
  type RampStep,
  rampBackground,
} from "./lib/accent";

/** A ramp step, the empty track (a "free" remainder), or no swatch at all. */
export type SwatchStep = RampStep | "track" | "none";

export interface LegendItem {
  label: string;
  value: string;
  step: SwatchStep;
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

/** The 8 px legend key: a ramp swatch, the track, or an empty slot. */
export function Swatch({ step, size = 8 }: { step: SwatchStep; size?: 7 | 8 }) {
  const dim = size === 8 ? "size-2" : "size-[7px]";
  if (step === "none")
    return <span aria-hidden className={cn(dim, "shrink-0")} />;
  return (
    <span
      aria-hidden
      className={cn(
        dim,
        "shrink-0 rounded-mark",
        step === "track" &&
          "bg-track shadow-[inset_0_0_0_1px_var(--color-border)]"
      )}
      style={
        step === "track" ? undefined : { background: rampBackground(step) }
      }
    />
  );
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
            <Swatch step={item.step} />
            {item.label}
            <span className="figures text-foreground">{item.value}</span>
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
          <Swatch step={item.step} />
          <span className="flex-1 font-normal text-fg-subtle">
            {item.label}
          </span>
          <span className="figures">{item.value}</span>
        </li>
      ))}
    </ul>
  );
}
