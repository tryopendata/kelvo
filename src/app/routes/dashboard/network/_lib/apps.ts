import { scopeWords, type TimeRange } from "@core/brush";
import type { NetworkByApp } from "@core/generated/bindings";

/** One row of the Apps table: a named app or one of the remainder rows. */
export interface AppRow {
  key: string;
  name: string;
  kind: "app" | "other" | "overhead" | "system";
  rxBytes: number;
  txBytes: number;
  totalBytes: number;
  /** Percent of the table's bytes; null when nothing moved. */
  share: number | null;
  /** Bytes/s over the latest closed 10 s bucket; null when unknown or not an app. */
  nowBps: number | null;
}

export type AppSortKey = "total" | "now";
export interface AppSort {
  by: AppSortKey;
  dir: "asc" | "desc";
}

/** "Apps, last 5 minutes" or "Apps, selected 90 s". */
export function appsTitle(
  selection: TimeRange | null,
  windowMs: number
): string {
  return `Apps, ${scopeWords(selection, windowMs)}`;
}

/**
 * Per-app rates over one bucket's answer: bytes over the time it measured.
 * Null when that bucket measured nothing (so "now" is unknown, not zero).
 */
export function nowRates(
  bucket: NetworkByApp | undefined
): Map<string, number> | null {
  if (!bucket || bucket.measured_ms <= 0) return null;
  const s = bucket.measured_ms / 1000;
  return new Map(
    bucket.apps.map((a) => [a.name, (a.rx_bytes + a.tx_bytes) / s])
  );
}

/**
 * What the shares are of: every row's bytes, so the column adds up to 100%.
 * That is the interface total, except when Rust reports the apps over it
 * (`clamped`), where dividing by the interface would put rows past 100%.
 */
function tableBytes(data: NetworkByApp): number {
  const remainder =
    data.other_apps_rx_bytes +
    data.other_apps_tx_bytes +
    data.overhead_rx_bytes +
    data.overhead_tx_bytes +
    data.system_rx_bytes +
    data.system_tx_bytes;
  return data.apps.reduce((n, a) => n + a.rx_bytes + a.tx_bytes, remainder);
}

const shareOf = (bytes: number, whole: number) =>
  whole > 0 ? (bytes / whole) * 100 : null;

/** The named apps as rows, with their "now" rate from `now`. */
export function appRows(
  data: NetworkByApp,
  now: Map<string, number> | null
): AppRow[] {
  const whole = tableBytes(data);
  return data.apps.map((a) => {
    const total = a.rx_bytes + a.tx_bytes;
    return {
      key: `app:${a.name}`,
      name: a.name,
      kind: "app",
      rxBytes: a.rx_bytes,
      txBytes: a.tx_bytes,
      totalBytes: total,
      share: shareOf(total, whole),
      nowBps: now === null ? null : (now.get(a.name) ?? 0),
    };
  });
}

/**
 * The rows after the apps, in order: "Other apps" when it moved
 * anything, then the estimated protocol overhead, then "System and other"
 * (always shown, so the table adds up to the interface).
 */
export function remainderRows(data: NetworkByApp): AppRow[] {
  const whole = tableBytes(data);
  const row = (
    kind: AppRow["kind"],
    name: string,
    rx: number,
    tx: number
  ): AppRow => ({
    key: kind,
    name,
    kind,
    rxBytes: rx,
    txBytes: tx,
    totalBytes: rx + tx,
    share: shareOf(rx + tx, whole),
    nowBps: null,
  });
  const out: AppRow[] = [];
  if (data.other_apps_rx_bytes + data.other_apps_tx_bytes > 0) {
    out.push(
      row(
        "other",
        "Other apps",
        data.other_apps_rx_bytes,
        data.other_apps_tx_bytes
      )
    );
  }
  out.push(
    row(
      "overhead",
      "Protocol overhead (est.)",
      data.overhead_rx_bytes,
      data.overhead_tx_bytes
    ),
    row(
      "system",
      "System and other",
      data.system_rx_bytes,
      data.system_tx_bytes
    )
  );
  return out;
}

/** Apps by the chosen column; unknown "now" sorts last, names break ties. */
export function sortApps(rows: readonly AppRow[], sort: AppSort): AppRow[] {
  const val = (r: AppRow) => (sort.by === "total" ? r.totalBytes : r.nowBps);
  const sign = sort.dir === "desc" ? -1 : 1;
  return [...rows].sort((a, b) => {
    const va = val(a);
    const vb = val(b);
    if (va === null || vb === null) {
      if (va !== vb) return va === null ? 1 : -1;
    } else if (va !== vb) {
      return (va - vb) * sign;
    }
    return a.name.localeCompare(b.name);
  });
}

/**
 * "Measured for 40 of 90 s" when the range was only partly recorded. Not
 * while the range reaches an open bucket: `measured_ms` includes the open
 * part, which is still filling, so the complete part's share of it is not
 * known. The note appears once those buckets close. A second or 1% of slack
 * absorbs sampling jitter.
 */
export function partialCoverage(
  data: NetworkByApp
): { measuredS: number; spanS: number } | null {
  if (openTailS(data) > 0) return null;
  const expected = data.to_ms - data.from_ms;
  if (expected <= 0) return null;
  const measured = data.measured_ms;
  if (measured <= 0) return null;
  const slack = Math.max(1000, expected * 0.01);
  if (measured >= expected - slack) return null;
  return {
    measuredS: Math.round(measured / 1000),
    spanS: Math.round(expected / 1000),
  };
}

/**
 * Seconds at the end of the range the engine is still measuring (its open
 * buckets), or 0 when every bucket is complete. Totals over that part still
 * grow, and its remainder is not meaningful until it closes.
 */
export function openTailS(data: NetworkByApp): number {
  const from = Math.max(data.from_ms, data.complete_to_ms);
  return data.to_ms > from ? Math.round((data.to_ms - from) / 1000) : 0;
}

/** Nothing in the range was recorded (before collection, or history off). */
export const unrecorded = (data: NetworkByApp) =>
  data.measured_ms <= 0 && data.apps.length === 0;

/** Start of the first recorded span, or null when none is recorded. */
export function firstRecordedMs(data: NetworkByApp): number | null {
  return data.coverage.find((c) => c.tier !== null)?.from_ms ?? null;
}

/** The whole window as complete 10 s buckets ending at the complete edge. */
export function windowRange(edgeMs: number, windowMs: number): TimeRange {
  return { fromMs: edgeMs - windowMs, toMs: edgeMs };
}
