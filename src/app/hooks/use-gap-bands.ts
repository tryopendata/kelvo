import { type Gap, type Module, RING_SPAN_MS } from "@core/generated/bindings";
import { probeHistory } from "@core/history-probe";
import { type GapBandSpec, gapBands } from "@core/history-state";
import { historyKeys } from "@core/query-keys";
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { useTransport } from "~/lib/transport-context";
import { useModuleRange } from "~/stores/chart-range";
import { useHost, useHostId, useHostStore } from "~/stores/host-store";

/** Refetch period: a gap that closed shows its end within this long. */
const REFRESH_MS = 60_000;
const NO_GAPS: readonly Gap[] = [];

/**
 * Gaps over the last hour, one `query_history` shared by every live chart
 * on the page (plan 4.17). Gaps are whole-host rows, so the selector only
 * has to be valid and `max_points: 1` keeps the points cheap. Fetched on
 * mount, every minute, and again when the stream pauses or
 * resumes, so an open "Paused" gap does not stay over the samples that
 * follow it. A sleep's band shows within a minute of waking; until then the
 * line already breaks there. History unavailable means no bands.
 */
function useRingGaps(windowMs: number, endMs: number | null): readonly Gap[] {
  const transport = useTransport();
  const hostId = useHostId();
  const store = useHostStore();
  const paused = useHost((s) => s.status?.paused ?? false);
  const started = useHost((s) => s.lastTsMs !== null);
  const { data } = useQuery({
    queryKey: historyKeys.ringGaps(hostId, paused, windowMs, endMs),
    queryFn: async () => {
      // The ring's clock: gaps and chart axes are both host time.
      const now = endMs ?? store.getState().lastTsMs ?? 0;
      const page = await probeHistory(transport, {
        host: hostId,
        metric: "cpu.total",
        fromMs: now - windowMs - REFRESH_MS,
        toMs: now,
        maxPoints: 1,
      });
      return page.gaps;
    },
    // Charts have no axis before the first row, and the query needs its time.
    enabled: started || endMs !== null,
    staleTime: REFRESH_MS,
    refetchInterval: endMs === null ? REFRESH_MS : false,
    placeholderData: (previous, query) =>
      endMs === null &&
      query?.queryKey[1] === hostId &&
      query.queryKey[4] === windowMs &&
      query.queryKey[5] === null
        ? previous
        : undefined,
  });
  return data ?? NO_GAPS;
}

/**
 * Labelled gap bands for a module page chart: whole-host gaps plus this
 * module's own (`module_disabled`), with clock-time labels ("Asleep
 * 11:02–11:31 · not interpolated"). Bands are not clipped here; an open gap
 * runs to `Number.MAX_SAFE_INTEGER` and each chart clips to its own x range.
 * Recomputed only when the gaps change, not every tick.
 */
export function useGapBands(
  module: Module,
  override?: { windowMs: number; endMs: number | null } | null
): GapBandSpec[] {
  const context = useModuleRange();
  const range = override ?? context;
  const gaps = useRingGaps(
    range?.windowMs ?? RING_SPAN_MS,
    range?.endMs ?? null
  );
  return useMemo(
    () => gapBands(gaps, 0, Number.MAX_SAFE_INTEGER, { module }),
    [gaps, module]
  );
}
