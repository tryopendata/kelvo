import type { HeatmapMetric } from "@core/generated/bindings";
import { heatmapDays } from "@core/heatmap-days";
import { HISTORY_COMMIT_MS } from "@core/history-state";
import { historyKeys } from "@core/query-keys";
import { type CommandFailure, unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { useNextBoundary } from "~/hooks/use-next-boundary";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";
import { currentCell, type HeatmapRow, heatmapRows } from "../_lib/heatmap";

export const HEATMAP_DAYS = 30;

/** The start of the local hour holding `ms`. */
export function localHourStart(ms: number): number {
  const d = new Date(ms);
  d.setMinutes(0, 0, 0);
  return d.getTime();
}

/**
 * The start of the current local hour, updated by one timer at the next
 * hour: the heatmap's only clock, so it re-renders once an hour and never
 * with the 1 Hz frames.
 */
export function useLocalHour(): number {
  return useNextBoundary(currentHourStart, nextHourStart);
}

const currentHourStart = () => localHourStart(Date.now());

function nextHourStart(hour: number): number {
  const next = new Date(hour);
  next.setHours(next.getHours() + 1);
  return next.getTime();
}

export interface HeatmapData {
  rows: HeatmapRow[];
  /** The cell holding now, `[row, hour]`. */
  current: [number, number] | null;
  /** The start of the current local hour: cells after it are still ahead. */
  hourMs: number;
  /** No cells for this metric yet: neither data nor gaps to draw. */
  loading: boolean;
  error: CommandFailure | null;
}

/**
 * Hourly averages over the last 30 local days (`query_heatmap`). The key
 * moves each local hour; within the hour it is read again every 5 minutes,
 * the writer's commit interval (D-070), so the current hour fills in as its
 * minutes land instead of staying empty until the next hour. Never per
 * tick. Switching back to a metric within 5 minutes is a cache hit.
 */
export function useHeatmap(metric: HeatmapMetric): HeatmapData {
  const transport = useTransport();
  const hostId = useHostId();
  const hour = useLocalHour();
  const specs = useMemo(
    () => heatmapDays(new Date(hour), HEATMAP_DAYS),
    [hour]
  );

  const query = useQuery({
    queryKey: historyKeys.heatmap(hostId, metric, hour, HEATMAP_DAYS),
    queryFn: () =>
      unwrap(transport.queryHeatmap({ host: hostId, metric, days: specs })),
    staleTime: HISTORY_COMMIT_MS,
    refetchInterval: HISTORY_COMMIT_MS,
    // The previous hour's cells stay up while the next hour's load; another
    // metric's do not, they would be drawn on the wrong scale.
    placeholderData: (prev, prevQuery) =>
      prevQuery?.queryKey[3] === metric ? prev : undefined,
  });

  const rows = useMemo(
    () => heatmapRows(specs, query.data),
    [specs, query.data]
  );
  const current = useMemo(() => currentCell(rows, hour), [rows, hour]);
  return {
    rows,
    current,
    hourMs: hour,
    loading: query.isPending,
    error: (query.error as CommandFailure | null) ?? null,
  };
}
