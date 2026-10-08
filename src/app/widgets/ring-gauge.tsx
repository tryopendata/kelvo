import { ringArcs } from "@core/chart-math";
import { NumberTicker } from "~/lib/motion/number-ticker";
import { cn } from "~/lib/utils";
import { type Accent, accentVars, rampColor } from "./lib/accent";

/** Sizes: 72 (Overview), 88 (Power rings), 112 (CPU clusters). */
export type RingSize = 72 | 88 | 112;

const GEOMETRY: Record<
  RingSize,
  { r: number; stroke: number; value: string; label: string; round: boolean }
> = {
  72: {
    r: 30,
    stroke: 6,
    value: "text-[14px]",
    label: "text-[9px]",
    round: false,
  },
  88: {
    r: 38,
    stroke: 7,
    value: "text-[17px]",
    label: "text-[10px]",
    round: true,
  },
  112: {
    r: 48,
    stroke: 8,
    value: "text-[18px]",
    label: "text-[10px]",
    round: true,
  },
};

export interface RingGaugeProps {
  /**
   * One or two segments as fractions of the full ring. The second starts where
   * the first ends and is drawn at ramp step 2. `null` is a missing
   * measurement: that segment draws no arc, leaving the track.
   */
  fractions: (number | null)[];
  /** Center figure, already formatted ("18%", "17.6", "3.20"). */
  value: string;
  /** Label under the figure ("load", "GB used", "GHz"). */
  label: string;
  accent: Accent;
  size: RingSize;
}

/**
 * Arc gauge with a centered value. Segments are full-circumference dashes
 * revealed by `stroke-dashoffset` and turned to their start angle, so a change
 * tweens offset and rotation (150 ms, off under reduced motion) and never the
 * dash pattern.
 */
export function RingGauge({
  fractions,
  value,
  label,
  accent,
  size,
}: RingGaugeProps) {
  const g = GEOMETRY[size];
  const c = size / 2;
  // A missing segment keeps its circle at zero length (the track shows), so
  // the arc can tween in when a value arrives.
  const { circumference, arcs } = ringArcs(
    g.r,
    fractions.slice(0, 2).map((f) => f ?? 0)
  );
  let start = 0;

  return (
    <div
      className="relative shrink-0"
      style={{ ...accentVars(accent), width: size, height: size }}
    >
      <svg
        width={size}
        height={size}
        viewBox={`0 0 ${size} ${size}`}
        aria-hidden
        className="-rotate-90"
      >
        <circle
          cx={c}
          cy={c}
          r={g.r}
          fill="none"
          stroke="var(--color-track)"
          strokeWidth={g.stroke}
        />
        {arcs.map((arc, i) => {
          const angle = (start / circumference) * 360;
          start += arc.length;
          return (
            <circle
              // Segments are positional: 0 is the headline series, 1 the second.
              // biome-ignore lint/suspicious/noArrayIndexKey: fixed two-slot list
              key={i}
              cx={c}
              cy={c}
              r={g.r}
              fill="none"
              stroke={i === 0 ? "var(--a)" : rampColor(2)}
              strokeWidth={g.stroke}
              strokeLinecap={g.round && arc.length > 0 ? "round" : "butt"}
              strokeDasharray={`${circumference} ${circumference}`}
              strokeDashoffset={circumference - arc.length}
              className="transition-[stroke-dashoffset,transform] duration-(--motion-tick) ease-tick"
              style={{
                transform: `rotate(${angle}deg)`,
                transformOrigin: `${c}px ${c}px`,
              }}
            />
          );
        })}
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center gap-px">
        <span className={cn("figures-display tracking-[-0.02em]", g.value)}>
          <NumberTicker text={value} unit={label} />
        </span>
        <span className={cn("font-normal text-muted-foreground", g.label)}>
          {label}
        </span>
      </div>
    </div>
  );
}
