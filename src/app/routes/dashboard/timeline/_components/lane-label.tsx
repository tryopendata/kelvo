import { TableIcon } from "lucide-react";
import { LANE_HEIGHT, type LaneDef } from "../_lib/lanes";
import type { LaneSummary } from "../_lib/summaries";

export interface LaneLabelProps {
  def: LaneDef;
  /** Fact line from the visible range ("peak 71% · 14:02"). */
  summary: LaneSummary;
  onShowTable: () => void;
}

/**
 * Label column describes the recorded interval; crosshair values remain momentary.
 */
export function LaneLabel({ def, summary, onShowTable }: LaneLabelProps) {
  return (
    <div
      className="group flex flex-col justify-center gap-px border-border-subtle border-r pr-3"
      style={{ height: LANE_HEIGHT }}
    >
      <div className="flex items-center gap-1.5">
        <span
          aria-hidden
          className="size-2 rounded-mark"
          style={{ background: `var(--color-${def.accent})` }}
        />
        <span className="font-[590] text-[12px]">{def.label}</span>
        <button
          type="button"
          onClick={onShowTable}
          aria-label={`Show ${def.label} as table`}
          title="Show as table"
          className="ml-auto inline-flex size-5 items-center justify-center rounded-chip text-muted-foreground opacity-0 outline-none transition-opacity hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring group-hover:opacity-100"
        >
          <TableIcon className="size-3" />
        </button>
      </div>
      <span className="figures-display text-[16px] leading-5 tracking-[-0.01em]">
        {summary.value}
      </span>
      <span className="figures truncate text-[10px] text-muted-foreground leading-3">
        {summary.detail}
      </span>
      {summary.coverage && (
        <span
          className="figures truncate text-[10px] text-fg-faint leading-3"
          title={summary.coverage}
        >
          {summary.coverage}
        </span>
      )}
    </div>
  );
}
