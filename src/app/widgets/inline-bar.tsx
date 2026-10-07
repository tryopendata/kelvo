import { clamp01 } from "@core/chart-math";
import { cn } from "~/lib/utils";
import {
  type Accent,
  accentVars,
  type RampStep,
  rampBackground,
} from "./lib/accent";
import { FIELD_LABEL } from "./lib/classes";

export interface InlineBarProps {
  label: string;
  /** Formatted value ("3.2 GHz", "42%"). */
  value: string;
  /**
   * Fill as a fraction of the track. `null` is a missing measurement: the
   * track is drawn empty, with no fill, never a zero-width fill that reads
   * as a real 0. `"none"` means the bar has no meaning here (a Mac without
   * fans): the value text stands alone and no track is drawn.
   */
  fraction: number | null | "none";
  /** Defaults to the accent in scope (the card's `--a`). */
  accent?: Accent;
  rampStep?: RampStep;
  /**
   * `stacked`: mono uppercase label and value over a 4 px bar (Overview, 04).
   * `row`: label, right-aligned value, bar in one grid row (popover, 14).
   * `wide`: label and value over a 6 px bar (popover memory pressure, battery).
   */
  layout?: "stacked" | "row" | "wide";
}

function Track({
  fraction,
  step,
  height,
}: {
  fraction: number | null;
  step: RampStep;
  height: "h-1" | "h-1.5";
}) {
  return (
    <span
      data-missing={fraction === null || undefined}
      className={cn("block overflow-hidden rounded-full bg-track", height)}
    >
      {fraction !== null && (
        <span
          className="block h-full origin-left rounded-full transition-transform duration-(--motion-tick) ease-tick"
          style={{
            background: rampBackground(step),
            transform: `scaleX(${clamp01(fraction)})`,
          }}
        />
      )}
    </span>
  );
}

/** Label, value and a horizontal bar. */
export function InlineBar({
  label,
  value,
  fraction,
  accent,
  rampStep = 1,
  layout = "stacked",
}: InlineBarProps) {
  const style = accent ? accentVars(accent) : undefined;

  if (layout === "row") {
    return (
      <div
        className="grid grid-cols-[64px_52px_minmax(0,1fr)] items-center gap-2 text-[12px]"
        style={style}
      >
        <span className="font-normal text-fg-subtle">{label}</span>
        <span
          className={cn(
            "data-mono text-right",
            fraction === null && "text-muted-foreground"
          )}
        >
          {value}
        </span>
        {fraction === "none" ? (
          <span />
        ) : (
          <Track fraction={fraction} step={rampStep} height="h-1" />
        )}
      </div>
    );
  }

  const wide = layout === "wide";
  return (
    <div className="flex flex-col gap-1" style={style}>
      <div className="flex justify-between gap-2">
        <span
          className={
            wide ? "font-normal text-[11px] text-fg-subtle" : FIELD_LABEL
          }
        >
          {label}
        </span>
        <span
          className={cn(
            "data-mono text-[11px]",
            typeof fraction !== "number" && "text-muted-foreground"
          )}
        >
          {value}
        </span>
      </div>
      {fraction === "none" ? (
        <span aria-hidden className={wide ? "h-1.5" : "h-1"} />
      ) : (
        <Track
          fraction={fraction}
          step={rampStep}
          height={wide ? "h-1.5" : "h-1"}
        />
      )}
    </div>
  );
}
