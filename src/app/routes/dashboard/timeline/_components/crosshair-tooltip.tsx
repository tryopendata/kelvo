import type { ProcessesAt } from "@core/generated/bindings";
import type { CSSProperties } from "react";
import {
  ChartTooltipShell,
  SeriesSwatch,
} from "~/components/charts/chart-tooltip";
import { InitialChip } from "~/widgets/initial-chip";
import type { Accent } from "~/widgets/lib/accent";
import { FIELD_LABEL } from "~/widgets/lib/classes";

export interface TooltipRow {
  label: string;
  value: string;
  accent: Accent;
}

export interface CrosshairTooltipProps {
  /** "Sun Oct 4 · 14:02:00". */
  title: string;
  /** "1 min avg" or "10 s avg". */
  resolution: string;
  rows: TooltipRow[];
  /** `undefined` while the query is in flight; `null` when nothing was stored then. */
  processes: ProcessesAt | null | undefined;
  /** A note in place of the rows, inside a gap ("Asleep 5h 15m · no samples"). */
  note?: string;
  style: CSSProperties;
}

const TOP = 5;

/**
 * The single crosshair tooltip: the moment and its resolution,
 * one row per lane with the bucket average, then the top processes stored
 * nearest that moment in % of one core.
 */
export function CrosshairTooltip({
  title,
  resolution,
  rows,
  processes,
  note,
  style,
}: CrosshairTooltipProps) {
  const top = [...(processes?.rows ?? [])]
    .sort((a, b) => (b.cpu_pct ?? -1) - (a.cpu_pct ?? -1))
    .slice(0, TOP);
  return (
    <ChartTooltipShell className="w-[276px]" style={style}>
      <div className="flex items-baseline justify-between">
        <span className="data-mono text-[12px]">{title}</span>
        <span className={FIELD_LABEL}>{resolution}</span>
      </div>
      {note ? (
        <span className="font-normal text-[12px] text-muted-foreground">
          {note}
        </span>
      ) : (
        <div className="flex flex-col gap-[5px]">
          {rows.map((r) => (
            <div key={r.label} className="flex items-center gap-2 text-[12px]">
              <SeriesSwatch color={`var(--color-${r.accent})`} />
              <span className="flex-1 font-normal text-fg-subtle">
                {r.label}
              </span>
              <span className="data-mono">{r.value}</span>
            </div>
          ))}
        </div>
      )}
      {!note && (
        <>
          <div className="h-px bg-border" />
          <div className="flex justify-between">
            <span className={FIELD_LABEL}>Top processes then</span>
            <span className={FIELD_LABEL}>% of 1 core</span>
          </div>
          {processes === undefined ? (
            <span className="font-normal text-[12px] text-muted-foreground">
              Looking up processes…
            </span>
          ) : top.length === 0 ? (
            <span className="font-normal text-[12px] text-muted-foreground">
              No processes stored near this time
            </span>
          ) : (
            <div className="flex flex-col gap-[5px]">
              {top.map((p) => (
                <div
                  key={`${p.pid}:${p.name}`}
                  className="flex items-center gap-2 text-[12px]"
                >
                  <InitialChip text={p.name} />
                  <span className="flex-1 truncate font-normal text-fg-subtle">
                    {p.name}
                  </span>
                  <span className="data-mono">
                    {p.cpu_pct === null ? "—" : `${Math.round(p.cpu_pct)}%`}
                  </span>
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </ChartTooltipShell>
  );
}
