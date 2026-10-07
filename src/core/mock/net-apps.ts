/**
 * Per-app network bytes for the mock transport's `query_network_by_app`
 * (D-089). The live `net.rx` / `net.tx` rows the mock streams are the source:
 * each row's interface bytes are split across apps, so whatever the Network
 * chart draws is what the Apps table attributes.
 *
 * Two recurring bursts are added on top of the walk (`NET_BURSTS`), each
 * charged to one app: a Docker Desktop download every 5 minutes, one of
 * which ends 60 s before the mock starts (so a fresh Network page shows it),
 * and a short-lived `curl`. The rest of each row is shared out with slowly
 * varying weights (Google Chrome, Safari, Claude Code, Dropbox, other apps);
 * header overhead is packets × 66 B as in Rust, and what is left is "System
 * and other", so both are never zero while bytes move.
 *
 * Per-app collection starts `NET_COLLECTION_DELAY_MS` after the ring's first
 * row, so a range reaching that far back is partly measured, and anything
 * older is "not recorded".
 *
 * The trailing buckets are open, as in the engine with no process view: the
 * per-app stream reports every 10 s on its own phase (`NET_APPS_PHASE_MS`
 * past the grid) while the interface reports every row, so rows from
 * `appsToMs` on count for the interface only, and `complete_to_ms` stops at
 * the bucket the per-app stream has not passed. Pure: the same rows give the
 * same answer.
 */
import {
  type AppBytes,
  CLAMP_SLACK_BYTES,
  HEADER_BYTES_PER_PACKET,
  MAX_NET_SPAN_MS,
  NET_BUCKET_MS,
  type NetworkByApp,
  type NetworkSpan,
} from "@core/generated/bindings";
import { ceilTo, floorTo } from "@core/time-grid";

/** Per-app history starts this long after the ring's first row. */
export const NET_COLLECTION_DELAY_MS = 40_000;
/** Rust's `MAX_NET_SPAN_MS`: the longest history retention. */
export const NET_MAX_SPAN_MS: number = MAX_NET_SPAN_MS;
/** The per-app stream samples this far past each 10 s edge (no process view). */
export const NET_APPS_PHASE_MS = 4_000;

/** How far the per-app stream has reported when the newest row is `lastTsMs`. */
export function appsReportedTo(lastTsMs: number): number {
  return (
    floorTo(lastTsMs - NET_APPS_PHASE_MS, NET_BUCKET_MS) + NET_APPS_PHASE_MS
  );
}
/** Bytes per interface packet the mock assumes, per direction. */
const RX_PACKET_BYTES = 1514;
const TX_PACKET_BYTES = 900;

/**
 * A burst of traffic one app causes, recurring every `periodMs`: active over
 * `[startMs + offsetMs + k * periodMs, … + durationMs)` for every integer k,
 * where `startMs` is the mock's start. Its rates are added to `net.rx` /
 * `net.tx` of en0.
 */
export interface NetBurst {
  app: string;
  offsetMs: number;
  periodMs: number;
  durationMs: number;
  rxBps: number;
  txBps: number;
}

export const NET_BURSTS: readonly NetBurst[] = [
  {
    app: "Docker Desktop",
    offsetMs: -90_000,
    periodMs: 300_000,
    durationMs: 30_000,
    rxBps: 80e6,
    txBps: 0.3e6,
  },
  {
    app: "curl",
    offsetMs: -200_000,
    periodMs: 420_000,
    durationMs: 6_000,
    rxBps: 12e6,
    txBps: 0.05e6,
  },
];

/** The burst windows of `burst` that overlap `[fromMs, toMs)`. */
export function burstWindows(
  burst: NetBurst,
  startMs: number,
  fromMs: number,
  toMs: number
): [number, number][] {
  const origin = startMs + burst.offsetMs;
  const out: [number, number][] = [];
  let k = Math.floor((fromMs - origin - burst.durationMs) / burst.periodMs);
  for (; ; k++) {
    const s = origin + k * burst.periodMs;
    if (s >= toMs) break;
    if (s + burst.durationMs > fromMs) out.push([s, s + burst.durationMs]);
  }
  return out;
}

/** The bursts' rates at `tMs`, per app. */
export function burstsAt(
  tMs: number,
  startMs: number
): { app: string; rxBps: number; txBps: number }[] {
  return NET_BURSTS.filter((b) => {
    const phase = (tMs - startMs - b.offsetMs) % b.periodMs;
    const p = phase < 0 ? phase + b.periodMs : phase;
    return p < b.durationMs;
  }).map((b) => ({ app: b.app, rxBps: b.rxBps, txBps: b.txBps }));
}

/**
 * The steady apps' shares of what the bursts leave, before headers. Each
 * swings ±25% on its own period, so the order shifts over an hour; at the
 * peaks they sum to under 0.9, leaving room for overhead and System.
 */
const STEADY: readonly {
  app: string | null;
  rx: number;
  tx: number;
  periodMs: number;
}[] = [
  { app: "Google Chrome", rx: 0.42, tx: 0.18, periodMs: 420_000 },
  { app: "Safari", rx: 0.14, tx: 0.06, periodMs: 610_000 },
  { app: "Claude Code", rx: 0.06, tx: 0.22, periodMs: 270_000 },
  { app: "Dropbox", rx: 0.05, tx: 0.2, periodMs: 930_000 },
  // "Other apps": the fold Rust reports apart from the named ones.
  { app: null, rx: 0.04, tx: 0.04, periodMs: 510_000 },
];

const swing = (tMs: number, periodMs: number, phase: number) =>
  1 + 0.25 * Math.sin((2 * Math.PI * tMs) / periodMs + phase);

/** One ring row as the per-app split sees it. */
export interface NetRow {
  ts: number;
  /** Interface rates summed over interfaces, bytes/s; null when unsampled. */
  rxBps: number | null;
  txBps: number | null;
}

interface Bucket {
  measuredMs: number;
  iface: [number, number];
  pkts: [number, number];
  apps: Map<string | null, [number, number]>;
}

const add = (
  m: Map<string | null, [number, number]>,
  k: string | null,
  rx: number,
  tx: number
) => {
  const cur = m.get(k) ?? [0, 0];
  m.set(k, [cur[0] + rx, cur[1] + tx]);
};

/** Rust's `split_direction`: overhead shrinks before the apps do. */
export function splitDirection(
  ifaceBytes: number,
  ifacePkts: number,
  attributedBytes: number
): { overhead: number; system: number; clamped: boolean } {
  const room = Math.max(0, ifaceBytes - attributedBytes);
  const overhead = Math.min(ifacePkts * HEADER_BYTES_PER_PACKET, room);
  return {
    overhead,
    system: room - overhead,
    clamped: attributedBytes > ifaceBytes + CLAMP_SLACK_BYTES,
  };
}

/**
 * One row's bytes, split into apps, into `b`. `apps` false: the per-app
 * stream has not reported it yet, so only the interface counts it.
 */
function addRow(
  b: Bucket,
  row: NetRow,
  dtMs: number,
  startMs: number,
  apps: boolean
) {
  if (row.rxBps === null || row.txBps === null) return;
  const dt = dtMs / 1000;
  const rx = Math.round(row.rxBps * dt);
  const tx = Math.round(row.txBps * dt);
  const pktRx = Math.ceil(rx / RX_PACKET_BYTES);
  const pktTx = Math.ceil(tx / TX_PACKET_BYTES);
  b.iface[0] += rx;
  b.iface[1] += tx;
  b.pkts[0] += pktRx;
  b.pkts[1] += pktTx;
  if (!apps) return;
  b.measuredMs += dtMs;
  // Apps see payload: their share of the bytes, less the share of headers.
  const payloadRx = 1 - HEADER_BYTES_PER_PACKET / RX_PACKET_BYTES;
  const payloadTx = 1 - HEADER_BYTES_PER_PACKET / TX_PACKET_BYTES;
  let leftRx = rx;
  let leftTx = tx;
  for (const burst of burstsAt(row.ts, startMs)) {
    const bRx = Math.min(leftRx, Math.round(burst.rxBps * dt));
    const bTx = Math.min(leftTx, Math.round(burst.txBps * dt));
    leftRx -= bRx;
    leftTx -= bTx;
    add(
      b.apps,
      burst.app,
      Math.floor(bRx * payloadRx),
      Math.floor(bTx * payloadTx)
    );
  }
  STEADY.forEach((s, i) => {
    const w = swing(row.ts, s.periodMs, i);
    add(
      b.apps,
      s.app,
      Math.floor(leftRx * s.rx * w * payloadRx),
      Math.floor(leftTx * s.tx * w * payloadTx)
    );
  });
}

export interface NetByAppInput {
  rows: readonly NetRow[];
  /** Each row's span: the sampling interval. */
  intervalMs: number;
  /** The mock's start (the bursts' phase). */
  startMs: number;
  /** Per-app collection began here; earlier rows are not recorded. */
  collectingFromMs: number;
  /** Spans with Network history off (`to` null: still off). */
  offSpans: readonly { from: number; to: number | null }[];
  /**
   * The per-app stream has reported rows before this; later ones are in
   * open buckets. Null when the engine holds no open buckets (history off,
   * nothing collected).
   */
  appsToMs: number | null;
  fromMs: number;
  toMs: number;
}

/** `query_network_by_app` over the mock's ring rows. */
export function mockNetworkByApp(input: NetByAppInput): NetworkByApp {
  const from = floorTo(input.fromMs, NET_BUCKET_MS);
  const to = Math.max(from, ceilTo(input.toMs, NET_BUCKET_MS));
  const off = (t: number) =>
    input.offSpans.some((s) => t >= s.from && (s.to === null || t < s.to));
  const buckets = new Map<number, Bucket>();
  for (const row of input.rows) {
    if (row.ts < from || row.ts >= to) continue;
    if (row.ts < input.collectingFromMs || off(row.ts)) continue;
    const key = floorTo(row.ts, NET_BUCKET_MS);
    let b = buckets.get(key);
    if (!b) {
      b = { measuredMs: 0, iface: [0, 0], pkts: [0, 0], apps: new Map() };
      buckets.set(key, b);
    }
    addRow(
      b,
      row,
      input.intervalMs,
      input.startMs,
      input.appsToMs === null || row.ts < input.appsToMs
    );
  }

  const coverage: NetworkSpan[] = [];
  const total: Bucket = {
    measuredMs: 0,
    iface: [0, 0],
    pkts: [0, 0],
    apps: new Map(),
  };
  for (let t = from; t < to; t += NET_BUCKET_MS) {
    const b = buckets.get(t);
    const tier = b && b.measuredMs > 0 ? "s10" : null;
    const last = coverage[coverage.length - 1];
    if (last && last.to_ms === t && last.tier === tier)
      last.to_ms = t + NET_BUCKET_MS;
    else coverage.push({ from_ms: t, to_ms: t + NET_BUCKET_MS, tier });
    if (!b) continue;
    total.measuredMs += Math.min(NET_BUCKET_MS, b.measuredMs);
    for (const d of [0, 1] as const) {
      total.iface[d] += b.iface[d];
      total.pkts[d] += b.pkts[d];
    }
    for (const [app, [rx, tx]] of b.apps) add(total.apps, app, rx, tx);
  }

  const other = total.apps.get(null) ?? [0, 0];
  const apps: AppBytes[] = [...total.apps]
    .filter((e): e is [string, [number, number]] => e[0] !== null)
    .map(([name, [rx, tx]]) => ({ name, rx_bytes: rx, tx_bytes: tx }))
    .filter((a) => a.rx_bytes + a.tx_bytes > 0)
    .sort(
      (a, b) =>
        b.rx_bytes + b.tx_bytes - (a.rx_bytes + a.tx_bytes) ||
        a.name.localeCompare(b.name)
    );
  const attributed = (d: 0 | 1) =>
    apps.reduce((n, a) => n + (d === 0 ? a.rx_bytes : a.tx_bytes), 0) +
    other[d];
  const rx = splitDirection(total.iface[0], total.pkts[0], attributed(0));
  const tx = splitDirection(total.iface[1], total.pkts[1], attributed(1));
  return {
    from_ms: from,
    to_ms: to,
    complete_to_ms:
      input.appsToMs === null
        ? to
        : Math.min(to, floorTo(input.appsToMs, NET_BUCKET_MS)),
    resolution_ms: NET_BUCKET_MS,
    measured_ms: total.measuredMs,
    coverage,
    apps,
    other_apps_rx_bytes: other[0],
    other_apps_tx_bytes: other[1],
    iface_rx_bytes: total.iface[0],
    iface_tx_bytes: total.iface[1],
    overhead_rx_bytes: rx.overhead,
    overhead_tx_bytes: tx.overhead,
    system_rx_bytes: rx.system,
    system_tx_bytes: tx.system,
    clamped: rx.clamped || tx.clamped,
  };
}
