import { clamp01 } from "@core/chart-math";
import { type Accent, accentVars } from "./lib/accent";

export interface ResidencyBarProps {
  /** "P-cluster". */
  cluster: string;
  activePct: number;
  /**
   * Frequency states, highest first, then "other" if any were merged, then
   * "idle" last. `pct` is percent of the window.
   */
  states: { label: string; pct: number }[];
  accent: Accent;
}

const IDLE = "idle";

/**
 * Accent strength for the i-th non-idle state of n: 100% for the top state
 * down to 30% for the lowest (four states get 1, .75, .52, .32).
 */
function stateMix(i: number, n: number): string {
  const t = n <= 1 ? 0 : i / (n - 1);
  return `color-mix(in srgb, var(--a) ${(100 - t * 70).toFixed(0)}%, transparent)`;
}

/** Cluster frequency residency as a stacked bar plus a table. */
export function ResidencyBar({
  cluster,
  activePct,
  states,
  accent,
}: ResidencyBarProps) {
  const busy = states.filter((s) => s.label !== IDLE);
  const color = (label: string) =>
    label === IDLE
      ? "var(--color-track)"
      : stateMix(
          busy.findIndex((s) => s.label === label),
          busy.length
        );

  return (
    <div className="flex flex-col gap-2" style={accentVars(accent)}>
      <div className="flex justify-between">
        <span className="text-[12px]">{cluster}</span>
        <span className="data-mono text-[11px] text-muted-foreground">
          active {Math.round(activePct)}%
        </span>
      </div>
      <div
        role="img"
        aria-label={`${cluster} residency: ${states.map((s) => `${s.label} ${Math.round(s.pct)}%`).join(", ")}`}
        className="flex h-2.5 gap-px overflow-hidden rounded-mark"
      >
        {states.map((s) =>
          clamp01(s.pct / 100) === 0 ? null : (
            <span
              key={s.label}
              style={{
                flexGrow: s.pct,
                flexBasis: 0,
                background: color(s.label),
              }}
            />
          )
        )}
      </div>
      <ul className="flex flex-col gap-[3px]">
        {states.map((s) => (
          <li key={s.label} className="flex items-center gap-2 text-[11px]">
            <span
              aria-hidden
              className="size-2 shrink-0 rounded-mark"
              style={{ background: color(s.label) }}
            />
            <span className="data-mono flex-1 text-fg-subtle">{s.label}</span>
            <span className="data-mono">{Math.round(s.pct)}%</span>
          </li>
        ))}
      </ul>
    </div>
  );
}
