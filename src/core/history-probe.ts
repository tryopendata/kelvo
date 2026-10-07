import type { HistoryPage, HostId } from "@core/generated/bindings";
import { type Transport, unwrap } from "@core/transport";

export interface HistoryProbe {
  host: HostId;
  /** An unlabelled series. */
  metric: string;
  fromMs: number;
  toMs: number;
  maxPoints: number;
}

/**
 * A one-series `query_history` on the auto tier, for reads that want a
 * maximum or the gaps rather than a chart: gaps are whole-host rows, so any
 * valid selector returns them, and a small `maxPoints` keeps the points cheap.
 * Rejects on a command error, like `unwrap`.
 */
export function probeHistory(
  transport: Transport,
  { host, metric, fromMs, toMs, maxPoints }: HistoryProbe
): Promise<HistoryPage> {
  return unwrap(
    transport.queryHistory({
      host,
      selectors: [{ metric, labels: [] }],
      from_ms: fromMs,
      to_ms: toMs,
      tier: "auto",
      max_points: maxPoints,
    })
  );
}
