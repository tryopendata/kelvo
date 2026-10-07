import { type Gap, type Module, RING_SPAN_MS } from "@core/generated/bindings";
import { probeHistory } from "@core/history-probe";
import { type GapBandSpec, gapBands } from "@core/history-state";
import { historyKeys } from "@core/query-keys";
import { keepPreviousData, useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { useTransport } from "~/lib/transport-context";
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
function useRingGaps(): readonly Gap[] {
  const transport = useTransport();
  const hostId = useHostId();
  const store = useHostStore();
  const paused = useHost((s) => s.status?.paused ?? false);
  const started = useHost((s) => s.lastTsMs !== null);
  const { data } = useQuery({
    queryKey: historyKeys.ringGaps(hostId, paused),
    queryFn: async () => {
      // The ring's clock: gaps and chart axes are both host time.
      const now = store.getState().lastTsMs ?? 0;
      const page = await probeHistory(transport, {
        host: hostId,
        metric: "cpu.total",
        fromMs: now - RING_SPAN_MS - REFRESH_MS,
        toMs: now,
        maxPoints: 1,
      });
      return page.gaps;
    },
    // Charts have no axis before the first row, and the query needs its time.
    enabled: started,
    staleTime: REFRESH_MS,
    refetchInterval: REFRESH_MS,
    placeholderData: keepPreviousData,
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
export function useGapBands(module: Module): GapBandSpec[] {
  const gaps = useRingGaps();
  return useMemo(
    () => gapBands(gaps, 0, Number.MAX_SAFE_INTEGER, { module }),
    [gaps, module]
  );
}
