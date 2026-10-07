import { floorTo, type TimeRange } from "@core/brush";
import { useBrush } from "~/stores/brush-store";
import { useHost } from "~/stores/host-store";

/**
 * What a brushable live chart reads of the selection (D-089): the range to
 * draw (a drag in progress, else the committed one) and the newest elapsed
 * 10 s edge, past which nothing can be selected. Both null when `enabled` is
 * false. The edge moves every 10 s and the range on a drag or a selection,
 * so neither re-renders the chart per tick.
 */
export function useChartBrush(enabled: boolean): {
  selected: TimeRange | null;
  elapsedEdge: number | null;
} {
  const elapsedEdge = useHost((s) =>
    enabled && s.lastTsMs !== null ? floorTo(s.lastTsMs) : null
  );
  const selected = useBrush((s) => (enabled ? (s.draft ?? s.range) : null));
  return { selected, elapsedEdge };
}
