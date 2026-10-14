import type {
  CommandError,
  LiveMsg,
  LiveProcess,
  PerformanceReason,
  ProcessSort,
  ProcessView,
  SeriesKey,
  SeriesSelector,
} from "@core/generated/bindings";
import { METRIC_KINDS, PERFORMANCE_VISIBLE_MS } from "@core/generated/bindings";
import type {
  ProcessSignalError,
  ProcessSignalResult,
} from "../process-signal";
import type { CommandResult } from "../transport";
import type { HistoryRow } from "./contracts";
import { holdMs, matches } from "./series";
export const ok = <T>(data: T): CommandResult<T> => ({ status: "ok", data });
export const err = (error: CommandError): CommandResult<never> => ({
  status: "error",
  error,
});
export const signalErr = (error: ProcessSignalError): ProcessSignalResult => ({
  status: "error",
  error,
});
export const MOCK_USER = "me";
export const MOCK_LOCAL_IPV4 = "192.168.1.24";
export const MOCK_PUBLIC_IP = "203.0.113.42";
export const RECENT_MS = 120_000;
export const EARLIER_CHUNK_ROWS = 600;
export const EARLIER_AFTER_MS = 1000;
export function framePeriodMs(
  intervalMs: number,
  minPeriodMs: number | null
): number {
  if (minPeriodMs === null || minPeriodMs <= intervalMs) return intervalMs;
  const ticks = Math.ceil(
    (minPeriodMs - Math.floor(intervalMs / 2)) / intervalMs
  );
  return Math.max(1, ticks) * intervalMs;
}
export function holdsOf(
  cadences: readonly number[],
  intervalMs: number
): number[] {
  return cadences.map((c) => holdMs(c * intervalMs));
}
export function layoutMsg(layoutNo: number, series: SeriesKey[]): LiveMsg {
  return {
    kind: "layout",
    layout_no: layoutNo,
    series,
    kinds: series.map(
      (k) => METRIC_KINDS[k.metric as keyof typeof METRIC_KINDS] ?? "gauge"
    ),
  };
}
export const PERFORMANCE_MIN_PERIOD_MS: number = PERFORMANCE_VISIBLE_MS;
export function effectiveMinPeriodMs(
  minPeriodMs: number | null,
  performance: PerformanceReason
): number | null {
  if (performance === "off") return minPeriodMs;
  return Math.max(minPeriodMs ?? 0, PERFORMANCE_MIN_PERIOD_MS);
}
export const project = <T>(values: readonly T[], pick: number[] | null): T[] =>
  pick === null ? [...values] : pick.map((i) => values[i] as T);
export function pickFor(
  keys: readonly SeriesKey[],
  series: readonly SeriesSelector[] | undefined
): number[] | null {
  if (!series) return null;
  const out: number[] = [];
  keys.forEach((k, i) => {
    if (series.some((s) => matches(k, s))) out.push(i);
  });
  return out;
}
export const last = (v: number | null) => v ?? Number.NEGATIVE_INFINITY;
export const sum = (a: number | null, b: number | null) =>
  a === null || b === null ? Number.NEGATIVE_INFINITY : a + b;
export const SORT_VALUE: Record<ProcessSort, (p: LiveProcess) => number> = {
  cpu: (p) => last(p.cpu_pct),
  memory: (p) => p.mem_bytes,
  threads: (p) => p.threads,
  wakeups: (p) => last(p.idle_wakeups_per_s),
  energy: (p) => last(p.energy),
  disk_read: (p) => last(p.disk_read_bps),
  disk_write: (p) => last(p.disk_write_bps),
  disk_total: (p) => sum(p.disk_read_bps, p.disk_write_bps),
  net_rx: (p) => last(p.net_rx_bps),
  net_tx: (p) => last(p.net_tx_bps),
  net_total: (p) => sum(p.net_rx_bps, p.net_tx_bps),
  gpu: (p) => last(p.gpu_pct),
};
export function selectProcesses(
  rows: readonly LiveProcess[],
  view: ProcessView
): LiveProcess[] {
  const keys: ProcessSort[] = view.sort.length > 0 ? view.sort : ["cpu"];
  // Descending; equal values (both last) compare equal rather than NaN.
  const by = (k: ProcessSort) => (a: LiveProcess, b: LiveProcess) => {
    const va = SORT_VALUE[k](a);
    const vb = SORT_VALUE[k](b);
    return vb > va ? 1 : vb < va ? -1 : 0;
  };
  if (view.limit === null) return [...rows];
  const picked = new Map<string, LiveProcess>();
  for (const k of keys) {
    for (const p of [...rows].sort(by(k)).slice(0, view.limit)) {
      picked.set(`${p.pid}:${p.start_time_us}`, p);
    }
  }
  return [...picked.values()].sort(by(keys[0] as ProcessSort));
}
export const STORE = { needsHistory: true } as const;
export const segments = (from: HistoryRow[]): HistoryRow[][] => {
  const out: HistoryRow[][] = [];
  let seg: HistoryRow[] = [];
  for (const r of from) {
    const prev = seg[seg.length - 1];
    if (
      prev &&
      (r.ts - prev.ts > r.interval * 1.5 ||
        r.interval !== prev.interval ||
        r.timeline !== prev.timeline ||
        r.holds.some((h, i) => h !== prev.holds[i]))
    ) {
      out.push(seg);
      seg = [];
    }
    seg.push(r);
  }
  if (seg.length > 0) out.push(seg);
  return out;
};
export const chunked = (seg: HistoryRow[]): HistoryRow[][] => {
  const out: HistoryRow[][] = [];
  for (let i = 0; i < seg.length; i += EARLIER_CHUNK_ROWS) {
    out.push(seg.slice(i, i + EARLIER_CHUNK_ROWS));
  }
  return out;
};
export const sumAt = (values: (number | null)[], idx: number[]) => {
  const present = idx
    .map((i) => values[i])
    .filter((v): v is number => v != null);
  return present.length === 0 ? null : present.reduce((a, b) => a + b, 0);
};
export const coverRates = (rs: HistoryRow[], rx: number[], tx: number[]) => {
  const out = rs.map((r) => ({
    ts: r.ts,
    rxBps: sumAt(r.values, rx),
    txBps: sumAt(r.values, tx),
  }));
  const hold = (r: HistoryRow) =>
    Math.max(0, ...rx.map((i) => r.holds[i] ?? 0));
  for (let i = out.length - 1; i >= 0; i--) {
    const cur = out[i];
    const src = rs[i];
    if (!cur || !src || cur.rxBps === null) continue;
    const reach = cur.ts - hold(src);
    for (let j = i - 1; j >= 0; j--) {
      const prev = out[j];
      if (!prev || prev.rxBps !== null || prev.ts < reach) break;
      prev.rxBps = cur.rxBps;
      prev.txBps = cur.txBps;
    }
  }
  return out;
};
