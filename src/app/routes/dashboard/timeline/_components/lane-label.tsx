import { TableIcon } from "lucide-react";
import { useHost } from "~/stores/host-store";
import { selectNetwork } from "~/stores/live-selectors";
import { formatMetric, type LaneUnits } from "../_lib/format";
import { LANE_HEIGHT, type LaneDef, type LaneId } from "../_lib/lanes";

/** The series whose current value the label shows. */
const NOW_METRIC: Record<LaneId, string> = {
  cpu: "cpu.total",
  gpu: "gpu.util",
  memory: "mem.pressure",
  power: "power.system",
  temp: "thermal.hottest",
  network: "net.rx_total",
};

/**
 * Upload plus download, summed over interfaces, as the sidebar shows it; null
 * when either direction is missing.
 */
export function networkTotal({
  rx,
  tx,
}: {
  rx: number | null;
  tx: number | null;
}): number | null {
  return rx === null || tx === null ? null : rx + tx;
}

export interface LaneLabelProps {
  def: LaneDef;
  /** Fact line from the visible range ("peak 71% · 14:02"). */
  sub: string;
  units: LaneUnits;
  onShowTable: () => void;
}

/**
 * Label column of one lane: swatch and name, the current value,
 * and a sub line. Subscribes to its one live value, so a tick re-renders
 * only this label.
 */
export function LaneLabel({ def, sub, units, onShowTable }: LaneLabelProps) {
  const now = useHost((s) =>
    def.id === "network"
      ? networkTotal(selectNetwork(s))
      : (s.held[NOW_METRIC[def.id]] ?? null)
  );
  return (
    <div
      className="group flex flex-col justify-center gap-[3px] border-border-subtle border-r pr-3"
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
      <span className="figures-display text-[16px] tracking-[-0.01em]">
        {formatMetric(NOW_METRIC[def.id], now, units)}
      </span>
      <span className="figures truncate text-[10px] text-muted-foreground">
        {sub}
      </span>
    </div>
  );
}
