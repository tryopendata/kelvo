import { pad2 } from "@core/format";
import { ChartAnnotation, type ChartAnnotationProps } from "./chart-annotation";
import { accentVars } from "./lib/accent";

export interface BatteryHistoryBarsProps {
  /** One entry per hour, oldest first. `tsMs` is the start of the hour. */
  hours: { tsMs: number; charge: number | null; charging: boolean }[];
  annotations: ChartAnnotationProps[];
  /** Plot height in px (140). */
  height?: number;
}

const HOUR_MS = 3_600_000;
const HATCH =
  "repeating-linear-gradient(135deg, var(--color-grid) 0 1px, transparent 1px 7px)";

function hourLabel(tsMs: number): string {
  return pad2(new Date(tsMs).getHours());
}

/**
 * Hourly battery charge over 24 hours: one bar per hour at the
 * charge at the end of that hour, a lime mark under hours that saw charging,
 * and every third hour labelled. An hour with no samples is hatched.
 */
export function BatteryHistoryBars({
  hours,
  annotations,
  height = 140,
}: BatteryHistoryBarsProps) {
  const n = Math.max(1, hours.length);
  const cols = { gridTemplateColumns: `repeat(${n}, minmax(0, 1fr))` };
  const first = hours[0]?.tsMs ?? 0;
  const span = n * HOUR_MS;
  const summary = hours
    .filter((h) => h.charge != null)
    .map((h) => `${hourLabel(h.tsMs)}:00 ${Math.round(h.charge ?? 0)}%`)
    .join(", ");
  return (
    <div className="relative pl-7.5" style={accentVars("battery")}>
      <div
        aria-hidden
        className="pointer-events-none absolute top-0 right-0 left-7.5 flex flex-col justify-between"
        style={{ height }}
      >
        <span className="h-px bg-grid" />
        <span className="h-px bg-grid" />
        <span className="h-px bg-grid" />
        <span className="h-px bg-axis" />
      </div>
      {[
        ["100", -5],
        ["67", height / 3 - 6],
        ["33", (2 * height) / 3 - 6],
      ].map(([label, top]) => (
        <span
          key={label}
          aria-hidden
          className="data-mono absolute left-0 text-[10px] text-fg-faint"
          style={{ top: top as number }}
        >
          {label}
        </span>
      ))}
      <div
        role="img"
        aria-label={`Battery charge at the end of each hour, last 24 hours: ${summary}`}
        className="relative grid items-end gap-1"
        style={{ ...cols, height }}
      >
        {hours.map((h) => (
          <div
            key={h.tsMs}
            title={`${hourLabel(h.tsMs)}:00 · ${h.charge == null ? "no samples" : `${Math.round(h.charge)}%`}${h.charging ? " · charging" : ""}`}
            data-gap={h.charge == null || undefined}
            className="rounded-t-mark"
            style={
              h.charge == null
                ? { height: "100%", background: HATCH }
                : {
                    height: `${Math.min(100, Math.max(0, h.charge))}%`,
                    background: `color-mix(in srgb, var(--a) ${h.charging ? 75 : 42}%, transparent)`,
                  }
            }
          />
        ))}
      </div>
      <div aria-hidden className="mt-1 grid gap-1" style={cols}>
        {hours.map((h) => (
          <div
            key={h.tsMs}
            className="h-1 rounded-full"
            style={{ background: h.charging ? "var(--a)" : "transparent" }}
          />
        ))}
      </div>
      <div aria-hidden className="mt-1.5 grid gap-1" style={cols}>
        {hours.map((h, i) => (
          <span
            key={h.tsMs}
            className="data-mono text-center text-[10px] text-fg-faint"
          >
            {i % 3 === 0 ? hourLabel(h.tsMs) : ""}
          </span>
        ))}
      </div>
      {annotations.map((a) => {
        const f = (a.tsMs - first) / span;
        if (f < 0 || f > 1) return null;
        return (
          <div
            key={`${a.tsMs}-${a.label}`}
            className="absolute top-5.5"
            style={{ left: `calc(30px + (100% - 30px) * ${f.toFixed(4)})` }}
          >
            <ChartAnnotation {...a} variant={a.variant ?? "pill"} />
          </div>
        );
      })}
    </div>
  );
}
