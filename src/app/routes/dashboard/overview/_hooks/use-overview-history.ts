import type { Module } from "@core/generated/bindings";
import { historyKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useHistoryHealth } from "~/hooks/use-history-health";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";
import { lastWakeMs, pageMax } from "../_lib/history";

const DAY_MS = 24 * 3_600_000;
/** Plan 4.5: the 24 h maxima refresh every 10 minutes. */
const REFRESH_MS = 10 * 60_000;

/**
 * 24 h maximum of `metric` (an unlabelled series), one `query_history`
 * on mount, refreshed every 10 minutes. Undefined while loading or when
 * history is unavailable: Rust then answers from the engine's last hour
 * alone, which is no 24 h maximum. Callers fall back to the bar's floor.
 */
export function useMax24h(module: Module, metric: string): number | undefined {
  const transport = useTransport();
  const hostId = useHostId();
  const unavailable = useHistoryHealth().error?.kind === "history_unavailable";
  const { data } = useQuery({
    queryKey: historyKeys.maxima(hostId, module, metric),
    queryFn: async () => {
      const now = Date.now();
      const page = await unwrap(
        transport.queryHistory({
          host: hostId,
          selectors: [{ metric, labels: [] }],
          from_ms: now - DAY_MS,
          to_ms: now,
          tier: "auto",
          max_points: 288,
        })
      );
      return pageMax(page);
    },
    staleTime: REFRESH_MS,
    refetchInterval: REFRESH_MS,
  });
  return unavailable ? undefined : (data ?? undefined);
}

/**
 * When the Mac last woke, from the sleep gaps of the last week. Null when
 * there is none in that span (or history is unavailable).
 */
export function useLastWake(): number | null {
  const transport = useTransport();
  const hostId = useHostId();
  const { data } = useQuery({
    queryKey: historyKeys.lastWake(hostId),
    queryFn: async () => {
      const now = Date.now();
      const page = await unwrap(
        transport.queryHistory({
          host: hostId,
          selectors: [{ metric: "cpu.total", labels: [] }],
          from_ms: now - 7 * DAY_MS,
          to_ms: now,
          tier: "auto",
          max_points: 1,
        })
      );
      return lastWakeMs(page.gaps);
    },
    staleTime: REFRESH_MS,
    refetchInterval: REFRESH_MS,
  });
  return data ?? null;
}
