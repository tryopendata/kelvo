import { floorTo, type TimeRange, windowRange } from "@core/brush";
import { useBrushRange } from "~/stores/brush-store";
import { useHost } from "~/stores/host-store";

/**
 * Start of the 10 s bucket the newest sample is in, or null before the
 * first sample. Changes once every 10 s, so a component reading it
 * re-renders at that cadence, not at 1 Hz. Always null while `enabled` is
 * false, so a reader that has it off does not re-render at all.
 */
export function useOpenEdge(enabled = true): number | null {
  return useHost((s) =>
    enabled && s.lastTsMs !== null ? floorTo(s.lastTsMs) : null
  );
}

export interface RangeScope {
  /** The brushed range, or null. */
  selection: TimeRange | null;
  /** What to read: the selection, else the chart window up to the open edge. */
  range: TimeRange | null;
  /** The answer may stand in while the key moves: only for the window view. */
  keepPrevious: boolean;
}

/**
 * The range a view over the chart reads (D-089, D-099): the selection when
 * there is one, otherwise the chart window (D-091) ending where the open
 * 10 s bucket starts, so its key moves every 10 s. `edgeMs` overrides that
 * end (Network's complete edge).
 */
export function useRangeScope(
  windowMs: number,
  edgeMs?: number | null
): RangeScope {
  const selection = useBrushRange();
  const open = useOpenEdge();
  const edge = edgeMs === undefined ? open : edgeMs;
  return {
    selection,
    range: selection ?? (edge === null ? null : windowRange(edge, windowMs)),
    keepPrevious: selection === null,
  };
}

/**
 * The answer to show: a held answer stands in only for the window it was
 * read for (the key moving every 10 s), never for a selection that was just
 * cleared, whose span differs.
 */
export function heldFor<T extends { from_ms: number; to_ms: number }>(
  q: { data: T | undefined; isPlaceholderData: boolean },
  windowMs: number
): T | undefined {
  return q.isPlaceholderData &&
    q.data &&
    q.data.to_ms - q.data.from_ms !== windowMs
    ? undefined
    : q.data;
}
