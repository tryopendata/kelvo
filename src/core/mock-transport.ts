/**
 * The mock transport: the browser dev server, Vitest and Playwright run the
 * app against it instead of Rust. It speaks the same protocol as the app
 * transport (D-049 message order, `{ status, data | error }` results) and
 * generates data with a seeded walk that lands on fixed sample values.
 *
 * Tests drive it explicitly: `tick()` produces one frame, `push(msg)` injects
 * any message. The dev server passes `autoTick` so it streams at 1 Hz.
 *
 * Scenarios (combinable): sleep-gap (the live backfill and history have a
 * sleep hole), unknown-chip (Sensors unsupported, no thermal or fan series),
 * no-battery (Battery not present), no-fans (passive cooling), paused
 * (Status paused, no frames), stale (frames stop with no status change),
 * history-unavailable (the store failed to open; history queries fail),
 * low-disk (10 s history paused), history-trimmed (the size limit trimmed
 * history 20 days back), history-locked / history-corrupt / history-too-new
 * (unavailable with that reason; Reset history works except when locked),
 * appstore (no Quit), clock-step (the auto-tick steps the clock back five
 * minutes after ten ticks), cpu-power-uncalibrated / cpu-power-seeded /
 * cpu-power-calibrated (`power.cpu_source` 1, 3 or 2, and no E-cluster
 * power series, as on the M3 Max), wide-layout (170 series, the perf gate),
 * hidden-resume (after ten ticks the window misses half an hour, then
 * resumes; the perf gate), no-process-network (`process_network` false:
 * no per-process network rates; `appstore` implies it), no-process-gpu
 * (`process_gpu` false: no per-process GPU time; `appstore` implies it).
 *
 * `query_network_by_app` splits the ring's own `net.rx` / `net.tx` rows into
 * apps (mock/net-apps.ts), so a spike on the Network chart is attributable:
 * en0 carries a Docker Desktop download (80 MB/s for 30 s every 5 minutes,
 * the first ending 60 s before the mock starts) and a short `curl` burst
 * (`NET_BURSTS`). Per-app history starts 40 s after the ring's first row, so
 * longer ranges are partly measured; with `process_network` false nothing is
 * recorded, and turning Network history off stops recording from then.
 *
 * Process rows carry network rates (`NET_RATES`) only while the window's
 * view asks for them (`network`) and the capability is there, as Rust
 * samples NetworkStatistics only then (D-081); otherwise they are null.
 * GPU time (`GPU_PCT`) works the same way with `gpu` (D-085).
 *
 * The live channel follows D-066: `series` projects layouts, rows and
 * frames; the last two minutes arrive before `subscribe_live` returns and
 * the rest as `backfill_earlier` chunks of 600 rows, newest first, one per
 * task after the first frame (or after 1 s without one). Process rows are
 * shaped and paced by the window's `ProcessView`, tied to its stream. While
 * the window is hidden or the display idle no frames go out; on resume the
 * missed span arrives as `backfill` messages of at most 600 rows in time
 * order, one per task, ahead of the next frame. A clock step bumps the
 * `timeline` and the ring drops the rows at or after the stepped time.
 *
 * The tick follows the sampling interval: `intervalMs` sets the starting
 * setting, and an `update_settings` that changes it re-times the auto-tick
 * and the `Status` the windows see, as the engine does.
 *
 * `process_signal` follows the plan's rules: the refuse list, `pid_reused` on
 * a start-time mismatch, `permission_denied` for a process owned by another
 * user than `MOCK_USER`, and a signalled process leaves later process rows.
 */
import type {
  CapabilitiesChanged,
  ChartWindow,
  CommandError,
  Event,
  EventRecorded,
  HistoryGrowth,
  HistoryHealth,
  HistoryHealthChanged,
  HistoryRequest,
  HostId,
  HostsChanged,
  LiveMsg,
  LiveProcess,
  LiveStatus,
  NavigateRequested,
  PerformanceReason,
  ProcessSort,
  ProcessView,
  SeriesKey,
  SeriesSelector,
  Settings,
  SettingsChanged,
  SettingsPatch,
  SettingsSnapshot,
  WindowAppearanceChanged,
} from "@core/generated/bindings";
import {
  HISTORY_TIERS,
  HOLD_FACTOR,
  METRIC_KINDS,
  PERFORMANCE_VISIBLE_MS,
  RING_SPAN_MS,
} from "@core/generated/bindings";
import { CHART_WINDOWS } from "./live-window";
import { mockEnergyByApp } from "./mock/energy";
import { mockEvents } from "./mock/events";
import {
  capabilities,
  defaultSettings,
  hostRecord,
  MOCK_HOST_ID,
  type ScenarioFlags,
  type ScenarioName,
  scenarioFlags,
  TOPOLOGY,
  withGpuPct,
  withNetRates,
  withPorts,
} from "./mock/fixtures";
import { MockGenerator } from "./mock/generator";
import { mockHeatmap } from "./mock/heatmap";
import {
  mockBatteryHours,
  mockGaps,
  mockHistory,
  mockNetworkTotals,
  mockRecentHistory,
  mockSeriesStats,
} from "./mock/history";
import {
  appsReportedTo,
  burstsAt,
  mockNetworkByApp,
  NET_COLLECTION_DELAY_MS,
  NET_MAX_SPAN_MS,
} from "./mock/net-apps";
import { mockUsageByApp } from "./mock/usage";
import type { ProcessSignalError, ProcessSignalResult } from "./process-signal";
import { samplingPlan } from "./sampling-plans";
import { INTERVALS_MS, SIZE_LIMITS_MB } from "./settings-patch";
import type { CommandResult, Transport, Unsubscribe } from "./transport";

export interface MockTransportOptions {
  scenarios?: readonly ScenarioName[];
  windowLabel?: string;
  /** Clock for frame timestamps. Defaults to `Date.now`. */
  now?: () => number;
  /** Stream frames on a real timer (dev server). Tests call `tick()`. */
  autoTick?: boolean;
  /** Rows of history generated up front (one per interval). Default 600. */
  historyRows?: number;
  /** Starting `sampling.interval_ms`. Default 1000. */
  intervalMs?: number;
  /** Starting `general.chart_window`. Default 15m. */
  chartWindow?: ChartWindow;
  /** Every `update_settings` answers `settings_not_saved`, as on a full disk. */
  settingsNotSaved?: boolean;
  /** `export_csv` answers `cancelled`, as if the save dialog was dismissed. */
  exportCancels?: boolean;
  /**
   * What `history_growth` answers: null before an hour is recorded. Defaults
   * to a measured 105-series Mac (36 MB fixed, 2.05 MB per minute day).
   */
  historyGrowth?: HistoryGrowth | null;
}

export interface MockCall {
  command: string;
  args: unknown[];
}

export interface MockTransport extends Transport {
  readonly kind: "mock";
  readonly flags: ScenarioFlags;
  /** Every command the app sent, in order. */
  readonly calls: MockCall[];
  /** Deliver one message to every live subscriber. */
  push(msg: LiveMsg): void;
  /** Generate one tick: a frame (unless paused or stale) and processes. */
  tick(): void;
  /** Number of live subscribers. */
  subscriberCount(): number;
  /** Emit `navigate-requested` as Rust does for an open dashboard. */
  requestNavigate(route: string): void;
  /** Change the store's health and emit `history-health-changed`. */
  setHistoryHealth(health: HistoryHealth): void;
  /**
   * Step the host's wall clock by `deltaMs` as the engine reports it
   * (D-064, D-066): a new `Layout` on every channel, and the next frame
   * carries the next `timeline` and the new `layout_no` at the stepped time.
   * The ring drops the rows at or after that time (it must stay in time
   * order); pending earlier chunks are dropped.
   */
  stepClock(deltaMs: number): void;
  /**
   * The window is shown (`true`) or hidden, as Rust hears it: hidden, no
   * frames go out; shown, each channel gets the span it missed (D-066).
   */
  setWindowVisible(visible: boolean): void;
  /**
   * macOS Low Power Mode turns on or off: Performance mode follows unless
   * the setting holds it on, with a status and a `window-appearance-changed`.
   */
  setLowPowerMode(on: boolean): void;
  /** Display sleep or screen lock: `display_idle` in the status, no frames. */
  setDisplayIdle(idle: boolean): void;
  /**
   * The collector's reads of series `key` (display form) fail, or recover:
   * each tick is a gap, and `held` goes null once the last sample is older
   * than its hold, as the engine does (D-047).
   */
  setReadFailing(key: string, failing: boolean): void;
  /**
   * Sample `ms` of time at once: the host's clock moves ahead `ms` and the
   * ring gets a row per interval over it. Only for a hidden window or an
   * idle display, which receive no frames.
   */
  fastForward(ms: number): void;
  /** Emit `hosts-changed` with the current host list. */
  emitHostsChanged(): void;
  /**
   * A detector or alert fires: the event joins what `query_events` returns
   * and `event-recorded` goes out. The engine publishes once the commit is
   * queued, so a real read can still miss it (D-083); the mock commits at
   * once, and a test that needs the miss holds `queryEvents` itself.
   */
  recordEvent(event: Event): void;
  /** The view and stream of the window's process interest, null when none. */
  processInterest(): { view: ProcessView; stream: number | null } | null;
  /** Stop the auto-tick timer and drop subscribers. */
  dispose(): void;
}

interface HistoryRow {
  ts: number;
  /** The engine's tick when it was sampled: 2 s in the background (D-094). */
  interval: number;
  timeline: number;
  values: (number | null)[];
  /** How long each series' sample stays current (D-090), generator order. */
  holds: number[];
}

/** One window's channel. */
interface LiveSub {
  onMsg: (msg: LiveMsg) => void;
  stream: number;
  /** Indices of the generator's series this channel carries; null for all. */
  pick: number[] | null;
  series: SeriesKey[];
  minPeriodMs: number | null;
  lastFrameMs: number | null;
  backfillMs: number;
  /** Time of the newest row or frame sent; a resume sends what is after it. */
  lastSentMs: number | null;
  /** The timeline of what was sent; a resume after a step sends the new one. */
  sentTimeline: number | null;
  /** `backfill_earlier` chunks still to send, newest first. */
  earlier: LiveMsg[];
  earlierStarted: boolean;
  timer: ReturnType<typeof setTimeout> | null;
  /** Resume messages (and frames behind them), delivered one per task. */
  queue: LiveMsg[];
  /** The last `holds` sent before a frame, joined, so a change resends it. */
  holdsSent: string | null;
  /** The last status sent, joined: Rust sends a status only when it changed. */
  statusSent: string | null;
  queueTimer: ReturnType<typeof setTimeout> | null;
}

/** What Rust sends before `subscribe_live` returns (`RECENT_MS`). */
export const RECENT_MS = 120_000;
/** Rows per `backfill_earlier` chunk. */
export const EARLIER_CHUNK_ROWS = 600;
/** Earlier chunks start this long after subscribe if no frame comes first. */
const EARLIER_AFTER_MS = 1000;

/** How often a channel with `minPeriodMs` gets frames (Rust's pacing). */
function framePeriodMs(intervalMs: number, minPeriodMs: number | null): number {
  if (minPeriodMs === null || minPeriodMs <= intervalMs) return intervalMs;
  const ticks = Math.ceil(
    (minPeriodMs - Math.floor(intervalMs / 2)) / intervalMs
  );
  return Math.max(1, ticks) * intervalMs;
}

/**
 * How long a sample stays current: `HOLD_FACTOR` times its period, the
 * engine's `hold_ms` (D-047, D-090).
 */
function holdMs(periodMs: number): number {
  return Math.floor((periodMs * HOLD_FACTOR.num) / HOLD_FACTOR.den);
}

/** The engine's holds for a row whose series are sampled every `cadences` ticks. */
function holdsOf(cadences: readonly number[], intervalMs: number): number[] {
  return cadences.map((c) => holdMs(c * intervalMs));
}

/** A layout as Rust sends it: each series' catalog kind beside it. */
function layoutMsg(layoutNo: number, series: SeriesKey[]): LiveMsg {
  return {
    kind: "layout",
    layout_no: layoutNo,
    series,
    kinds: series.map(
      (k) => METRIC_KINDS[k.metric as keyof typeof METRIC_KINDS] ?? "gauge"
    ),
  };
}

/** Rust's floor on a channel's frame period in Performance mode (D-088). */
const PERFORMANCE_MIN_PERIOD_MS: number = PERFORMANCE_VISIBLE_MS;

/** A channel's pacing: what it asked for, raised to 2 s in Performance mode. */
function effectiveMinPeriodMs(
  minPeriodMs: number | null,
  performance: PerformanceReason
): number | null {
  if (performance === "off") return minPeriodMs;
  return Math.max(minPeriodMs ?? 0, PERFORMANCE_MIN_PERIOD_MS);
}

const project = <T>(values: readonly T[], pick: number[] | null): T[] =>
  pick === null ? [...values] : pick.map((i) => values[i] as T);

function matches(key: SeriesKey, sel: SeriesSelector): boolean {
  return (
    key.metric === sel.metric &&
    sel.labels.every(([k, v]) =>
      key.labels.some(([kk, vv]) => kk === k && vv === v)
    )
  );
}

/** Indices of `keys` any selector matches, in layout order. */
function pickFor(
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

/**
 * `sort_value` in `procview.rs`: a value that is not a number (null here:
 * NaN or not sampled) ranks last, and a sum is not a number when either
 * part is not.
 */
const last = (v: number | null) => v ?? Number.NEGATIVE_INFINITY;
const sum = (a: number | null, b: number | null) =>
  a === null || b === null ? Number.NEGATIVE_INFINITY : a + b;
const SORT_VALUE: Record<ProcessSort, (p: LiveProcess) => number> = {
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

/**
 * The rows a `ProcessView` asks for, as Rust picks them (D-066): every row
 * with no limit; else the union of the top `limit` by each sort key,
 * deduplicated, sorted by the first key.
 */
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

const signalErr = (error: ProcessSignalError): ProcessSignalResult => ({
  status: "error",
  error,
});
/** The user the mock app runs as; other users' processes answer EPERM. */
export const MOCK_USER = "me";
/** The mock's addresses: documentation ranges (RFC 1918, RFC 5737). */
export const MOCK_LOCAL_IPV4 = "192.168.1.24";
export const MOCK_PUBLIC_IP = "203.0.113.42";
const ok = <T>(data: T): CommandResult<T> => ({ status: "ok", data });
const err = (error: CommandError): CommandResult<never> => ({
  status: "error",
  error,
});

export function createMockTransport(
  options: MockTransportOptions = {}
): MockTransport {
  const flags = scenarioFlags(options.scenarios ?? ["default"]);
  const now = options.now ?? (() => Date.now());
  const label = options.windowLabel ?? "dashboard";
  const gen = new MockGenerator(flags);
  const calls: MockCall[] = [];
  const startMs = now();
  const baseSettings = defaultSettings(flags);
  const startInterval = options.intervalMs ?? 1000;
  gen.intervalMs = startInterval;

  // History the ring would hold: one row per interval ending now, on the
  // sample values. The sleep-gap scenario cuts a 15 s hole into the last
  // minute, so a fresh popover backfill arrives in two segments.
  const historyRows = options.historyRows ?? 600;
  // A window opened three minutes ago; before that only the tray was open,
  // so the adaptive collectors' history is every 10 s (D-061).
  const generated = gen.backfill(historyRows, Math.max(0, historyRows - 180));
  // The recurring bursts `query_network_by_app` charges to one app ride on
  // en0's rates, so the chart shows what the Apps table attributes.
  const en0 = (metric: string) =>
    gen.keys.findIndex(
      (k) =>
        k.metric === metric &&
        k.labels.some(([key, v]) => key === "iface" && v === "en0")
    );
  const en0Rx = en0("net.rx");
  const en0Tx = en0("net.tx");
  // The totals carry en0's bursts too, as the engine sums what it reports.
  const rxTotal = gen.keys.findIndex((k) => k.metric === "net.rx_total");
  const txTotal = gen.keys.findIndex((k) => k.metric === "net.tx_total");
  const withBursts = (ts: number, values: (number | null)[]) => {
    for (const b of burstsAt(ts, startMs)) {
      for (const [i, bps] of [
        [en0Rx, b.rxBps],
        [rxTotal, b.rxBps],
        [en0Tx, b.txBps],
        [txTotal, b.txBps],
      ] as const) {
        const v = values[i];
        if (v != null) values[i] = v + bps;
      }
    }
    return values;
  };
  const ringStartMs = startMs - (historyRows - 1) * startInterval;
  let rows: HistoryRow[] = generated.map((row, i) => {
    const ts = startMs - (historyRows - 1 - i) * startInterval;
    return {
      ts,
      interval: startInterval,
      timeline: 0,
      values: withBursts(ts, row.values),
      holds: holdsOf(row.cadences, startInterval),
    };
  });
  if (flags.sleepGap) {
    rows = rows.filter(
      (r) => r.ts < startMs - 40_000 || r.ts > startMs - 25_000
    );
  }
  // The ring holds nothing inside a closed history gap (the sleep
  // within the last hour), as the engine's ring would not.
  const closedGaps = mockGaps(flags, startMs).filter((g) => g.end_ms !== null);
  rows = rows.filter(
    (r) => !closedGaps.some((g) => r.ts >= g.start_ms && r.ts < (g.end_ms ?? 0))
  );

  let settings: SettingsSnapshot = {
    revision: 1,
    settings: {
      ...baseSettings,
      sampling: { ...baseSettings.sampling, interval_ms: startInterval },
      general: {
        ...baseSettings.general,
        chart_window: options.chartWindow ?? baseSettings.general.chart_window,
      },
    },
  };
  let status: LiveStatus = {
    interval_ms: startInterval,
    frame_period_ms: startInterval,
    paused: flags.paused,
    display_idle: false,
    on_battery: !flags.noBattery,
    // As the engine resolves it: on battery wins; a Mac without one is on the
    // adapter (D-092). The mock never charges.
    power_source: flags.noBattery ? "adapter" : "battery",
    // The default route is on en0, the reported Wi-Fi interface; on a
    // full-tunnel VPN it is on the tunnel, which is not reported.
    primary_iface: flags.vpn ? null : "en0",
    performance: "off",
  };
  let lowPowerMode = flags.lowPowerMode;
  // The engine's resolution: the setting wins over Low Power Mode (D-088).
  const performanceReason = (): PerformanceReason =>
    settings.settings.sampling.performance_mode
      ? "setting"
      : lowPowerMode
        ? "low_power_mode"
        : "off";
  status = { ...status, performance: performanceReason() };
  // What Rust hears about the window and the display (D-066).
  let windowVisible = true;
  /**
   * The engine's tick, from the sampling plans (`effective_interval_ms`), or
   * the background's while the window is hidden (D-094).
   */
  const effectiveInterval = () => {
    const s = settings.settings.sampling;
    const plan = samplingPlan({
      interval_ms: s.interval_ms,
      slow_on_battery: s.slow_on_battery,
      performance: !!s.performance_mode || lowPowerMode,
      low_power_mode: lowPowerMode,
    });
    if (!plan) return s.interval_ms;
    const f = status.on_battery ? plan.battery : plan.ac;
    return windowVisible ? f.tick_ms : f.background_tick_ms;
  };
  // Backed off from the start, as the engine is.
  const startTick = effectiveInterval();
  if (startTick !== startInterval) {
    status = { ...status, interval_ms: startTick, frame_period_ms: startTick };
  }
  const caps = capabilities(flags, gen.seriesPerModule());
  let interest: { view: ProcessView; stream: number | null } | null = null;
  let lastBatchMs: number | null = null;
  // The host's wall clock relative to `now` (`stepClock`), and its layout.
  let clockOffset = 0;
  let layoutNo = 1;
  let timeline = 0;
  const receiving = () => windowVisible && !status.display_idle;
  let ticks = 0;
  let nextStream = 1;
  let currentStream: number | null = null;
  let unavailable: CommandError | null = flags.historyUnavailable
    ? { kind: "history_unavailable", reason: flags.historyReason }
    : null;
  // When Network history was off (D-089): nothing is recorded then.
  const netHistoryOff: { from: number; to: number | null }[] = [];
  if (baseSettings.history.network_history === false) {
    netHistoryOff.push({ from: Number.NEGATIVE_INFINITY, to: null });
  }
  const netIndices = (metric: string) =>
    gen.keys.flatMap((k, i) => (k.metric === metric ? [i] : []));
  const rxIdx = netIndices("net.rx");
  const txIdx = netIndices("net.tx");
  /** A row's rate summed over interfaces; null when none was sampled. */
  const sumAt = (values: (number | null)[], idx: number[]) => {
    const present = idx
      .map((i) => values[i])
      .filter((v): v is number => v != null);
    return present.length === 0 ? null : present.reduce((a, b) => a + b, 0);
  };
  // Rust counts per-app bytes from counters, so a slow interface sample loses
  // nothing: its rate is the mean over the span since the previous sample
  // (D-090). Spread each rate back over the rows its hold covers.
  const coverRates = (rs: HistoryRow[], rx: number[], tx: number[]) => {
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
  /** A history read as `history_through_now` answers it, unavailable or not. */
  const historyPage = (req: HistoryRequest) =>
    unavailable
      ? mockRecentHistory(req, gen.specs, now(), status.interval_ms)
      : mockHistory(
          req,
          gen.specs,
          mockGaps(flags, startMs),
          now(),
          status.interval_ms
        );
  // Processes quit through `process_signal`, by pid; they leave later rows.
  const signalled = new Set<number>();
  const processRows = () =>
    gen.processes().filter((p) => !signalled.has(p.pid));
  let historyBytes = 148_000_000;
  let growth: HistoryGrowth | null =
    options.historyGrowth === undefined
      ? {
          measured_ms: 7 * 86_400_000,
          fixed_bytes: 36_000_000,
          minute_day_bytes: 2_050_000,
        }
      : options.historyGrowth;
  let health: HistoryHealth = {
    low_disk_paused: flags.lowDisk,
    trimmed_before_ms: flags.historyTrimmed ? startMs - 20 * 86_400_000 : null,
    trimmed_limit_bytes: flags.historyTrimmed ? 150_000_000 : null,
    cap_met: true,
  };

  const live = new Map<(msg: LiveMsg) => void, LiveSub>();
  const settingsListeners = new Set<(e: SettingsChanged) => void>();
  const capsListeners = new Set<(e: CapabilitiesChanged) => void>();
  const appearanceListeners = new Set<(e: WindowAppearanceChanged) => void>();
  const navigateListeners = new Set<(e: NavigateRequested) => void>();
  const healthListeners = new Set<(e: HistoryHealthChanged) => void>();
  const hostsListeners = new Set<(e: HostsChanged) => void>();
  const eventListeners = new Set<(e: EventRecorded) => void>();
  const events: Event[] = mockEvents(startMs);

  const record = (command: string, ...args: unknown[]) => {
    calls.push({ command, args });
  };

  /** Raw messages to every channel, unprojected (tests). */
  const push = (msg: LiveMsg) => {
    for (const cb of [...live.keys()]) cb(msg);
  };

  /** This channel's status: the frame period follows its minimum period. */
  const statusFor = (sub: LiveSub): LiveMsg => ({
    kind: "status",
    ...status,
    frame_period_ms: framePeriodMs(
      status.interval_ms,
      effectiveMinPeriodMs(sub.minPeriodMs, status.performance)
    ),
  });
  /** A status to `sub` if it differs from the last one it got. */
  const sendStatus = (sub: LiveSub) => {
    const msg = statusFor(sub);
    const key = JSON.stringify(msg);
    if (sub.statusSent === key) return;
    sub.statusSent = key;
    sub.onMsg(msg);
  };
  /** A hidden window's stream is stopped: it gets the status on resume. */
  const pushStatus = () => {
    if (!windowVisible) return;
    for (const sub of [...live.values()]) sendStatus(sub);
  };

  /**
   * Rows split where the ring has a hole (sleep, pause, skipped ticks) or
   * the timeline changes.
   */
  const segments = (from: HistoryRow[]): HistoryRow[][] => {
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

  const rowsMsg = (
    kind: "backfill" | "backfill_earlier",
    seg: HistoryRow[],
    sub: LiveSub
  ): LiveMsg => {
    const first = seg[0] as HistoryRow;
    const body = {
      layout_no: layoutNo,
      start_ms: first.ts,
      interval_ms: first.interval,
      rows: seg.map((r) => project(r.values, sub.pick)),
      holds_ms: project(first.holds, sub.pick),
    };
    if (kind === "backfill_earlier") return { kind, ...body };
    const last = seg[seg.length - 1] as HistoryRow;
    sub.lastSentMs = Math.max(sub.lastSentMs ?? last.ts, last.ts);
    sub.sentTimeline = last.timeline;
    return { kind, timeline: first.timeline, ...body };
  };

  /** Rows in pieces of at most `EARLIER_CHUNK_ROWS`, oldest first. */
  const chunked = (seg: HistoryRow[]): HistoryRow[][] => {
    const out: HistoryRow[][] = [];
    for (let i = 0; i < seg.length; i += EARLIER_CHUNK_ROWS) {
      out.push(seg.slice(i, i + EARLIER_CHUNK_ROWS));
    }
    return out;
  };

  /**
   * A frame, or anything else that must not overtake a resume: queued behind
   * the resume messages still waiting, delivered directly otherwise.
   */
  const deliver = (sub: LiveSub, msg: LiveMsg) => {
    if (sub.queue.length === 0) {
      sub.onMsg(msg);
      return;
    }
    sub.queue.push(msg);
  };

  /** One queued message per task, as a Tauri Channel delivers each. */
  const drainQueue = (sub: LiveSub) => {
    if (sub.queueTimer !== null) return;
    const next = () => {
      sub.queueTimer = null;
      const msg = sub.queue.shift();
      if (!msg || !live.has(sub.onMsg)) return;
      sub.onMsg(msg);
      if (sub.queue.length > 0) sub.queueTimer = setTimeout(next, 0);
    };
    sub.queueTimer = setTimeout(next, 0);
  };

  /**
   * Shown again or the display woke (D-066): the span each channel missed,
   * at most its backfill window, in time order and in chunks of at most 600
   * rows, one per task, ahead of the next frame.
   */
  const resume = () => {
    // Rust's catch-up sends the status in effect first (deduped), so whatever
    // changed while hidden (pause, power) arrives on show. The open-time
    // sample Rust may take (D-094) is not modelled: rows stay on the grid.
    for (const sub of live.values()) sendStatus(sub);
    const newest = rows[rows.length - 1]?.ts;
    if (newest === undefined) return;
    for (const sub of live.values()) {
      const floor = newest - sub.backfillMs;
      const held = sub.sentTimeline;
      // After a clock step: the rows not on the window's timeline.
      const stepped = held !== null && held !== timeline;
      const after = sub.lastSentMs ?? Number.NEGATIVE_INFINITY;
      const missed = rows.filter(
        (r) => r.ts > floor && (stepped ? r.timeline !== held : r.ts > after)
      );
      for (const seg of segments(missed)) {
        for (const chunk of chunked(seg)) {
          sub.queue.push(rowsMsg("backfill", chunk, sub));
        }
      }
      if (sub.queue.length > 0) drainQueue(sub);
    }
  };

  /**
   * The subscribe-time history as Rust sends it (D-066): the last two
   * minutes as `backfill` now, the rest of `backfillMs` as earlier chunks of
   * at most 600 rows, newest first. "Now" is the newest ring row.
   */
  const history = (sub: LiveSub, backfillMs: number) => {
    const newest = rows[rows.length - 1]?.ts ?? now() + clockOffset;
    const inWindow = rows.filter((r) => r.ts > newest - backfillMs);
    const recentFrom = newest - RECENT_MS;
    const recent = segments(inWindow.filter((r) => r.ts > recentFrom));
    const older = segments(inWindow.filter((r) => r.ts <= recentFrom));
    const chunks: HistoryRow[][] = [];
    for (const seg of older) {
      for (let end = seg.length; end > 0; end -= EARLIER_CHUNK_ROWS) {
        chunks.push(seg.slice(Math.max(0, end - EARLIER_CHUNK_ROWS), end));
      }
    }
    // Newest first: segments are oldest first and chunk each from its end.
    chunks.sort((a, b) => (b[0] as HistoryRow).ts - (a[0] as HistoryRow).ts);
    return {
      recent: recent.map((seg) => rowsMsg("backfill", seg, sub)),
      earlier: chunks.map((seg) => rowsMsg("backfill_earlier", seg, sub)),
      earlierRows: chunks.reduce((n, c) => n + c.length, 0),
      earlierStart: chunks.length
        ? (chunks[chunks.length - 1]?.[0]?.ts ?? null)
        : null,
    };
  };

  /**
   * Earlier chunks go out one per task, as a Tauri Channel delivers each
   * message: the webview gets a turn between chunks.
   */
  const sendEarlier = (sub: LiveSub) => {
    if (sub.earlierStarted) return;
    sub.earlierStarted = true;
    const next = () => {
      sub.timer = null;
      const chunk = sub.earlier.shift();
      if (!chunk || !live.has(sub.onMsg)) return;
      sub.onMsg(chunk);
      sub.timer = setTimeout(next, 0);
    };
    if (sub.timer !== null) clearTimeout(sub.timer);
    sub.timer = setTimeout(next, 0);
  };

  const interestActive = () =>
    interest !== null &&
    (interest.stream === null || interest.stream === currentStream);

  const sendProcesses = (ts: number) => {
    if (!interest || !interestActive()) return;
    const period = effectiveMinPeriodMs(
      interest.view.period_ms,
      status.performance
    );
    if (
      period !== null &&
      lastBatchMs !== null &&
      ts - lastBatchMs < period * 0.9
    ) {
      return;
    }
    lastBatchMs = ts;
    const net =
      interest.view.network === true && caps.process_network === true
        ? withNetRates(processRows())
        : processRows();
    const gpu =
      interest.view.gpu === true && caps.process_gpu === true
        ? withGpuPct(net)
        : net;
    const all = interest.view.ports === true ? withPorts(gpu) : gpu;
    const batch = selectProcesses(all, interest.view);
    push({ kind: "processes", ts_ms: ts, rows: batch });
  };

  /** One ring row at `ts`; a row not after the newest drops the overlap. */
  const pushRow = (
    ts: number,
    values: (number | null)[],
    cadences: readonly number[]
  ) => {
    while (rows.length > 0 && (rows[rows.length - 1] as HistoryRow).ts >= ts) {
      rows.pop();
    }
    rows.push({
      ts,
      interval: status.interval_ms,
      timeline,
      values,
      holds: holdsOf(cadences, status.interval_ms),
    });
    if (rows.length > 3600) rows = rows.slice(-3600);
  };

  const tick = () => {
    if (status.paused || flags.stale) return;
    ticks++;
    if (flags.clockStep && ticks === 11) transport.stepClock(-300_000);
    if (flags.hiddenResume && ticks === 11) {
      transport.setWindowVisible(false);
      transport.fastForward(30 * 60_000);
      // Shown in a later task, so anything the hidden window was sent
      // renders before it is shown, as it would sit in a real webview.
      setTimeout(() => {
        transport.setWindowVisible(true);
        // The perf gate measures long tasks from here: building the messages
        // above is Rust's work, applying them is the webview's.
        performance.mark("kelvo:resume");
      }, 50);
    }
    const ts = now() + clockOffset;
    // Adaptive collectors idle unless a window is taking frames.
    gen.trayOnly = !receiving();
    gen.intervalMs = status.interval_ms;
    const { values, held, cadences } = gen.next();
    withBursts(ts, values);
    withBursts(ts, held);
    pushRow(ts, values, cadences);
    const holds = (rows[rows.length - 1] as HistoryRow).holds;
    if (!receiving()) return;
    for (const sub of [...live.values()]) {
      const period = effectiveMinPeriodMs(sub.minPeriodMs, status.performance);
      const due =
        period === null ||
        sub.lastFrameMs === null ||
        ts - sub.lastFrameMs >= period - status.interval_ms / 2;
      if (!due) continue;
      sub.lastFrameMs = ts;
      sub.lastSentMs = ts;
      sub.sentTimeline = timeline;
      // A thinned channel's frames are a frame period apart: no hold is
      // shorter than that period's, as Rust sends it.
      const floor = holdMs(
        framePeriodMs(
          status.interval_ms,
          effectiveMinPeriodMs(sub.minPeriodMs, status.performance)
        )
      );
      const frameHolds = project(holds, sub.pick).map((h) =>
        Math.max(h, floor)
      );
      const joined = frameHolds.join(",");
      if (sub.holdsSent !== joined) {
        sub.holdsSent = joined;
        deliver(sub, {
          kind: "holds",
          layout_no: layoutNo,
          holds_ms: frameHolds,
        });
      }
      deliver(sub, {
        kind: "frame",
        ts_ms: ts,
        layout_no: layoutNo,
        timeline,
        values: project(values, sub.pick),
        held: project(held, sub.pick),
      });
      sendEarlier(sub);
    }
    sendProcesses(ts);
  };

  let timer: ReturnType<typeof setInterval> | null = null;
  const ensureTimer = () => {
    if (!options.autoTick || timer !== null) return;
    timer = setInterval(tick, status.interval_ms);
  };
  const retime = () => {
    if (timer === null) return;
    clearInterval(timer);
    timer = null;
    ensureTimer();
  };

  const emitSettings = () => {
    for (const cb of [...settingsListeners]) cb(settings);
  };

  /**
   * Re-resolve the tick and Performance mode after a settings or power
   * change: a status on either, and an appearance when the mode changed.
   */
  const updatePower = () => {
    const performance = performanceReason();
    const interval_ms = effectiveInterval();
    if (
      performance === status.performance &&
      interval_ms === status.interval_ms
    ) {
      return;
    }
    const retimed = interval_ms !== status.interval_ms;
    const changed = performance !== status.performance;
    status = { ...status, performance, interval_ms };
    pushStatus();
    if (retimed) retime();
    if (!changed) return;
    const appearance = {
      performance,
      reduce_transparency: false,
      theme: settings.settings.general.appearance,
    };
    for (const cb of [...appearanceListeners]) cb({ appearance });
  };

  const unknownHost = (host: HostId) =>
    host === MOCK_HOST_ID ? null : err({ kind: "unknown_host", host });

  const transport: MockTransport = {
    kind: "mock",
    flags,
    calls,
    windowLabel: () => label,
    push,
    tick,
    subscriberCount: () => live.size,
    requestNavigate(route) {
      for (const cb of [...navigateListeners]) cb({ route });
    },
    setHistoryHealth(next) {
      health = next;
      for (const cb of [...healthListeners]) cb({ health });
    },
    dispose() {
      if (timer !== null) clearInterval(timer);
      timer = null;
      for (const sub of live.values()) {
        if (sub.timer !== null) clearTimeout(sub.timer);
        if (sub.queueTimer !== null) clearTimeout(sub.queueTimer);
      }
      live.clear();
      hostsListeners.clear();
      settingsListeners.clear();
      capsListeners.clear();
      appearanceListeners.clear();
      navigateListeners.clear();
      healthListeners.clear();
      eventListeners.clear();
    },

    async listHosts() {
      record("list_hosts");
      return [hostRecord(flags, startMs)];
    },
    async getHost(host) {
      record("get_host", host);
      return unknownHost(host) ?? ok(hostRecord(flags, startMs));
    },
    async getCapabilities(host) {
      record("get_capabilities", host);
      return unknownHost(host) ?? ok(caps);
    },
    async subscribeLive(host, onMsg, opts = {}) {
      const backfillMs = Math.min(opts.backfillMs ?? 60_000, RING_SPAN_MS);
      record("subscribe_live", host, backfillMs, opts.series ?? null);
      const bad = unknownHost(host);
      if (bad) return { info: bad, unsubscribe: () => {} };
      const pick = pickFor(gen.keys, opts.series);
      const sub: LiveSub = {
        onMsg,
        stream: nextStream++,
        pick,
        series: project(gen.keys, pick),
        minPeriodMs: opts.minPeriodMs ?? null,
        lastFrameMs: null,
        backfillMs,
        lastSentMs: null,
        sentTimeline: null,
        earlier: [],
        earlierStarted: false,
        timer: null,
        queue: [],
        queueTimer: null,
        holdsSent: null,
        statusSent: null,
      };
      live.set(onMsg, sub);
      // A new subscription from the window ends interest tagged with another.
      currentStream = sub.stream;
      const h = history(sub, backfillMs);
      sub.earlier = h.earlier;
      onMsg({ kind: "caps", capabilities: caps });
      // A hidden window gets its status on show, never the background's.
      if (windowVisible) sendStatus(sub);
      onMsg(layoutMsg(layoutNo, sub.series));
      for (const msg of h.recent) onMsg(msg);
      ensureTimer();
      // Rust starts the earlier chunks after the first frame, or after 1 s
      // when no frame comes (paused, stale).
      if (sub.earlier.length > 0) {
        sub.timer = setTimeout(() => sendEarlier(sub), EARLIER_AFTER_MS);
      }
      const first = h.recent[0];
      return {
        info: ok({
          host,
          stream: sub.stream,
          backfill_start_ms: first?.kind === "backfill" ? first.start_ms : null,
          backfill_rows: h.recent.reduce(
            (n, m) => n + (m.kind === "backfill" ? m.rows.length : 0),
            0
          ),
          earlier_start_ms: h.earlierStart,
          earlier_rows: h.earlierRows,
        }),
        unsubscribe: () => {
          if (sub.timer !== null) clearTimeout(sub.timer);
          if (sub.queueTimer !== null) clearTimeout(sub.queueTimer);
          live.delete(onMsg);
        },
      };
    },
    async setProcessInterest(host, interested, view, stream) {
      record("set_process_interest", host, interested, view, stream);
      const bad = unknownHost(host);
      if (bad) return bad;
      const wasActive = interestActive();
      const wasNetwork = interest?.view.network === true;
      const wasGpu = interest?.view.gpu === true;
      // A new call replaces the window's view (D-066).
      interest = interested
        ? { view: view ?? { limit: null, sort: [], period_ms: null }, stream }
        : null;
      // One batch right away, so a page opened with `?ticks=0` (screenshots)
      // still has rows; Rust sends its next batch on the next sample. The
      // same when a view starts asking for network rates or GPU time (a
      // column set switched), so the values show without a tick.
      const startsNetwork = interest?.view.network === true && !wasNetwork;
      const startsGpu = interest?.view.gpu === true && !wasGpu;
      if (
        interestActive() &&
        (!wasActive || startsNetwork || startsGpu) &&
        !status.paused &&
        !flags.stale
      ) {
        lastBatchMs = null;
        queueMicrotask(() => sendProcesses(now() + clockOffset));
      }
      return ok(null);
    },
    stepClock(deltaMs) {
      clockOffset += deltaMs;
      layoutNo++;
      timeline++;
      for (const sub of live.values()) {
        sub.earlier = [];
        if (sub.timer !== null) clearTimeout(sub.timer);
        sub.timer = null;
        sub.lastFrameMs = null;
        sub.holdsSent = null;
        sub.onMsg(layoutMsg(layoutNo, sub.series));
      }
      lastBatchMs = null;
    },
    setWindowVisible(visible) {
      if (windowVisible === visible) return;
      const was = receiving();
      windowVisible = visible;
      // Rust's order (D-094): the tick changes before the stream resumes and
      // after it stops, so the window never gets the background's status.
      updatePower();
      if (!was && receiving()) resume();
    },
    setLowPowerMode(on) {
      lowPowerMode = on;
      updatePower();
    },
    setReadFailing(key, failing) {
      const i = gen.indexOf(key);
      if (i < 0) return;
      if (failing) gen.failing.add(i);
      else gen.failing.delete(i);
    },
    setDisplayIdle(idle) {
      if (status.display_idle === idle) return;
      const was = receiving();
      status = { ...status, display_idle: idle };
      pushStatus();
      if (!was && receiving()) resume();
    },
    fastForward(ms) {
      const interval = status.interval_ms;
      const last = rows[rows.length - 1]?.ts ?? now() + clockOffset;
      clockOffset += ms;
      // Time passes with the window hidden: only the tray is open.
      gen.trayOnly = true;
      gen.intervalMs = interval;
      for (
        let ts = last + interval;
        ts <= now() + clockOffset;
        ts += interval
      ) {
        const row = gen.next();
        pushRow(ts, withBursts(ts, row.values), row.cadences);
      }
      gen.trayOnly = !receiving();
    },
    emitHostsChanged() {
      const hosts = [hostRecord(flags, startMs)];
      for (const cb of [...hostsListeners]) cb({ hosts });
    },
    processInterest: () => (interestActive() ? interest : null),
    async queryHistory(request) {
      record("query_history", request);
      const bad = unknownHost(request.host);
      if (bad) return bad;
      if (request.selectors.length === 0 || request.to_ms < request.from_ms) {
        return err({
          kind: "invalid_argument",
          message: "empty selectors or negative span",
        });
      }
      // With history unavailable Rust answers from the engine's recent rows (`HISTORY_RECENT_MS`).
      if (unavailable) {
        return ok(
          mockRecentHistory(request, gen.specs, now(), status.interval_ms)
        );
      }
      // Gaps sit where the ring has its holes: fixed at start, not sliding.
      return ok(
        mockHistory(
          request,
          gen.specs,
          mockGaps(flags, startMs),
          now(),
          status.interval_ms
        )
      );
    },
    async queryProcessesAt(host, tMs) {
      record("query_processes_at", host, tMs);
      const bad = unknownHost(host);
      if (bad) return bad;
      return ok({
        ts_ms: Math.floor(tMs / 10_000) * 10_000,
        resolution: "snapshot",
        rows: processRows().map((p) => ({
          name: p.name,
          pid: p.pid,
          cpu_pct: p.cpu_pct,
          mem_bytes: p.mem_bytes,
          threads: p.threads,
          idle_wakeups_per_s: p.idle_wakeups_per_s,
          energy: p.energy,
        })),
      });
    },
    async queryNetworkByApp(host, fromMs, toMs) {
      record("query_network_by_app", host, fromMs, toMs);
      const bad = unknownHost(host);
      if (bad) return bad;
      // With history unavailable Rust answers from the engine's ring alone,
      // which is what the mock's rows are, so `unavailable` changes nothing.
      if (toMs < fromMs) {
        return err({
          kind: "invalid_argument",
          message: "the range ends before it starts",
        });
      }
      if (toMs - fromMs > NET_MAX_SPAN_MS) {
        return err({
          kind: "invalid_argument",
          message: "the range is longer than history keeps",
        });
      }
      const lastTs = rows[rows.length - 1]?.ts;
      const historyOn = netHistoryOff[netHistoryOff.length - 1]?.to !== null;
      return ok(
        mockNetworkByApp({
          // The engine's open buckets: none with history off or no
          // NetworkStatistics, else from the per-app stream's last report.
          appsToMs:
            caps.process_network && historyOn && lastTs !== undefined
              ? appsReportedTo(lastTs)
              : null,
          rows: coverRates(rows, rxIdx, txIdx),
          intervalMs: status.interval_ms,
          startMs,
          // No NetworkStatistics: nothing is ever recorded (appstore).
          collectingFromMs: caps.process_network
            ? ringStartMs + NET_COLLECTION_DELAY_MS
            : Number.POSITIVE_INFINITY,
          offSpans: netHistoryOff,
          fromMs,
          toMs,
        })
      );
    },

    async queryEnergyByApp(host, fromMs, toMs) {
      record("query_energy_by_app", host, fromMs, toMs);
      const bad = unknownHost(host);
      if (bad) return bad;
      if (toMs < fromMs) {
        return err({
          kind: "invalid_argument",
          message: "the range ends before it starts",
        });
      }
      const latestMs = rows[rows.length - 1]?.ts ?? startMs;
      return ok(
        mockEnergyByApp({
          processes: processRows(),
          user: MOCK_USER,
          fromMs,
          toMs,
          // The engine keeps an hour; the mock's ring is its whole run.
          sinceMs: Math.max(ringStartMs, latestMs - 3_600_000),
          latestMs,
        })
      );
    },
    async queryUsageByApp(host, fromMs, toMs, by, limit) {
      record("query_usage_by_app", host, fromMs, toMs, by, limit);
      const bad = unknownHost(host);
      if (bad) return bad;
      if (toMs < fromMs) {
        return err({
          kind: "invalid_argument",
          message: "the range ends before it starts",
        });
      }
      const latestMs = rows[rows.length - 1]?.ts ?? startMs;
      // The engine keeps an hour; the mock's ring is its whole run.
      const sinceMs = Math.max(ringStartMs, latestMs - 3_600_000);
      const gpu = caps.process_gpu === true;
      const from = Math.max(fromMs, sinceMs);
      const stats =
        from < toMs
          ? mockSeriesStats(
              host,
              ["cpu.total", "gpu.util", "disk.read_total", "disk.write_total"],
              from,
              toMs,
              now(),
              historyPage
            )
          : null;
      return ok(
        mockUsageByApp({
          processes: gpu ? withGpuPct(processRows()) : processRows(),
          user: MOCK_USER,
          fromMs,
          toMs,
          sinceMs,
          latestMs,
          by,
          limit,
          gpu,
          stats,
          cores: TOPOLOGY.reduce((n, c) => n + c.cores.length, 0),
        })
      );
    },
    async querySeriesStats(host, metrics, fromMs, toMs) {
      record("query_series_stats", host, metrics, fromMs, toMs);
      const bad = unknownHost(host);
      if (bad) return bad;
      if (toMs < fromMs) {
        return err({
          kind: "invalid_argument",
          message: "the range ends before it starts",
        });
      }
      if (toMs - fromMs > NET_MAX_SPAN_MS) {
        return err({
          kind: "invalid_argument",
          message: "the range is longer than history keeps",
        });
      }
      return ok(
        mockSeriesStats(host, metrics, fromMs, toMs, now(), historyPage)
      );
    },
    async getNetworkAddresses(host) {
      record("get_network_addresses", host);
      const bad = unknownHost(host);
      if (bad) return bad;
      return ok({
        iface: status.primary_iface,
        ipv4: status.primary_iface === null ? [] : [MOCK_LOCAL_IPV4],
        ipv6: [],
      });
    },
    async getPublicIp() {
      record("get_public_ip");
      return ok(MOCK_PUBLIC_IP);
    },

    async queryEvents(host, fromMs, toMs) {
      record("query_events", host, fromMs, toMs);
      const bad = unknownHost(host);
      if (bad) return bad;
      if (unavailable) return err(unavailable);
      return ok(events.filter((e) => e.ts_ms >= fromMs && e.ts_ms < toMs));
    },
    recordEvent(event) {
      events.push(event);
      events.sort((a, b) => a.ts_ms - b.ts_ms);
      for (const cb of [...eventListeners]) cb({ host: MOCK_HOST_ID, event });
    },

    async queryHeatmap(request) {
      record("query_heatmap", request);
      const bad = unknownHost(request.host);
      if (bad) return bad;
      if (unavailable) return err(unavailable);
      if (request.days.some((d) => d.hour_starts.length !== 25)) {
        return err({
          kind: "invalid_argument",
          message: "every day needs 25 hour boundaries",
        });
      }
      return ok(mockHeatmap(request, now()));
    },
    async queryNetworkTotals(host, fromMs, toMs) {
      record("query_network_totals", host, fromMs, toMs);
      const bad = unknownHost(host);
      if (bad) return bad;
      if (toMs < fromMs) {
        return err({
          kind: "invalid_argument",
          message: "the range ends before it starts",
        });
      }
      if (toMs - fromMs > NET_MAX_SPAN_MS) {
        return err({
          kind: "invalid_argument",
          message: "the range is longer than history keeps",
        });
      }
      return ok(mockNetworkTotals(host, fromMs, toMs, now(), historyPage));
    },
    async batteryHours(host, hourStartsMs) {
      record("battery_hours", host, hourStartsMs);
      const bad = unknownHost(host);
      if (bad) return bad;
      const hours = mockBatteryHours(host, hourStartsMs, (req) =>
        unavailable
          ? mockRecentHistory(req, gen.specs, now(), status.interval_ms)
          : mockHistory(
              req,
              gen.specs,
              mockGaps(flags, startMs),
              now(),
              status.interval_ms
            )
      );
      return typeof hours === "string"
        ? err({ kind: "invalid_argument", message: hours })
        : ok(hours);
    },
    async exportCsv(request) {
      record("export_csv", request);
      const bad = unknownHost(request.host);
      if (bad) return bad;
      if (unavailable) return err(unavailable);
      if (request.selectors.length === 0 || request.to_ms <= request.from_ms) {
        return err({
          kind: "invalid_argument",
          message: "empty selectors or empty range",
        });
      }
      if (options.exportCancels) return ok({ kind: "cancelled" });
      // One row per bucket of the tier `auto` would read (D-076), gaps apart.
      const span = request.to_ms - request.from_ms;
      const [s10, m1, m15] = HISTORY_TIERS;
      const tierMs =
        request.tier === "s10"
          ? s10.bucket_ms
          : request.tier === "m1"
            ? m1.bucket_ms
            : span <= s10.kept_ms
              ? s10.bucket_ms
              : span <= m1.kept_ms
                ? m1.bucket_ms
                : m15.bucket_ms;
      const gapRows = mockGaps(flags, startMs).filter(
        (g) =>
          g.start_ms < request.to_ms && (g.end_ms ?? Infinity) > request.from_ms
      ).length;
      const rows = Math.ceil(span / tierMs);
      return ok({
        kind: "saved",
        path: `/Users/mock/Downloads/${request.file_name ?? "kelvo-history.csv"}`,
        rows,
        gap_rows: gapRows,
        bytes: 64 + rows * 24 * (1 + request.selectors.length * 3),
      });
    },

    async getSettings() {
      record("get_settings");
      return settings;
    },
    async updateSettings(patch) {
      record("update_settings", patch);
      if (options.settingsNotSaved) {
        return err({ kind: "settings_not_saved", message: "disk full" });
      }
      const next = applyPatch(settings.settings, patch);
      const chartWindow = next.general.chart_window;
      if (chartWindow !== undefined && !CHART_WINDOWS.includes(chartWindow)) {
        return err({
          kind: "invalid_settings",
          message: `chart window ${chartWindow}`,
        });
      }
      if (
        !(INTERVALS_MS as readonly number[]).includes(next.sampling.interval_ms)
      ) {
        return err({
          kind: "invalid_settings",
          message: `interval ${next.sampling.interval_ms} ms`,
        });
      }
      if (
        !(SIZE_LIMITS_MB as readonly number[]).includes(
          next.history.size_limit_mb
        )
      ) {
        return err({
          kind: "invalid_settings",
          message: `size limit ${next.history.size_limit_mb} MB`,
        });
      }
      if (JSON.stringify(next) === JSON.stringify(settings.settings)) {
        return ok(settings);
      }
      const wasOn = settings.settings.history.network_history !== false;
      const isOn = next.history.network_history !== false;
      if (wasOn !== isOn) {
        const t = now() + clockOffset;
        const open = netHistoryOff[netHistoryOff.length - 1];
        if (!isOn) netHistoryOff.push({ from: t, to: null });
        else if (open && open.to === null) open.to = t;
      }
      settings = { revision: settings.revision + 1, settings: next };
      updatePower();
      emitSettings();
      return ok(settings);
    },
    async historySize(host) {
      record("history_size", host);
      return (
        unknownHost(host) ?? (unavailable ? err(unavailable) : ok(historyBytes))
      );
    },
    async historyGrowth(host) {
      record("history_growth", host);
      return unknownHost(host) ?? (unavailable ? err(unavailable) : ok(growth));
    },
    async clearHistory(host) {
      record("clear_history", host);
      const bad = unknownHost(host);
      if (bad) return bad;
      if (unavailable) return err(unavailable);
      historyBytes = 0;
      growth = null;
      if (health.trimmed_before_ms !== null) {
        transport.setHistoryHealth({
          ...health,
          trimmed_before_ms: null,
          trimmed_limit_bytes: null,
          cap_met: true,
        });
      }
      return ok(historyBytes);
    },
    async historyHealth(host) {
      record("history_health", host);
      const bad = unknownHost(host);
      if (bad) return bad;
      if (unavailable) return err(unavailable);
      return ok(health);
    },
    async resetHistory() {
      record("reset_history");
      // Another Kelvo process holds the lock: a reset cannot take it.
      if (
        unavailable?.kind === "history_unavailable" &&
        unavailable.reason?.kind === "locked"
      ) {
        return err({
          kind: "store_busy",
          message: "history.sqlite is locked by another Kelvo process",
        });
      }
      unavailable = null;
      historyBytes = 4096;
      growth = null;
      transport.setHistoryHealth({
        low_disk_paused: health.low_disk_paused,
        trimmed_before_ms: null,
        trimmed_limit_bytes: null,
        cap_met: true,
      });
      return ok(historyBytes);
    },
    async setPaused(paused) {
      record("set_paused", paused);
      if (status.paused === paused) return;
      status = { ...status, paused };
      pushStatus();
    },
    async openDashboard(route) {
      record("open_dashboard", route);
      return ok(null);
    },
    async sensorDump(host) {
      record("sensor_dump", host);
      const bad = unknownHost(host);
      if (bad) return bad;
      const info = hostRecord(flags, startMs).info;
      const last = rows[rows.length - 1]?.values ?? [];
      return ok({
        model: info.model,
        chip: info.chip,
        os_version: info.os_version,
        chip_known: info.chip_known,
        capabilities: caps,
        sensors: gen.specs
          .map((s, i) => ({ s, value: last[i] ?? null }))
          .filter(({ s }) => s.module === "power" || s.module === "sensors")
          .map(({ s, value }) => ({ key: s.key, value })),
      });
    },
    async processSignal(host, pid, startTimeUs, kind) {
      record("process_signal", host, pid, startTimeUs, kind);
      if (host !== MOCK_HOST_ID)
        return signalErr({ kind: "unknown_host", host });
      if (flags.appstore) return signalErr({ kind: "unavailable" });
      const p = processRows().find((row) => row.pid === pid);
      if (!p) return signalErr({ kind: "not_found" });
      if (p.start_time_us !== startTimeUs)
        return signalErr({ kind: "pid_reused" });
      if (p.refusal) return signalErr({ kind: "refused", refusal: p.refusal });
      if (p.user !== MOCK_USER) return signalErr({ kind: "permission_denied" });
      signalled.add(pid);
      return { status: "ok", data: null };
    },
    async getEdition() {
      record("get_edition");
      return { process_signal: !flags.appstore };
    },
    async checkForUpdates() {
      record("check_for_updates");
      return settings.settings.general.check_updates
        ? { kind: "not_configured" }
        : { kind: "disabled" };
    },
    async getWindowAppearance() {
      record("get_window_appearance");
      return {
        performance: status.performance,
        reduce_transparency: false,
        theme: settings.settings.general.appearance,
      };
    },
    async openUrl(url) {
      record("open_url", url);
    },
    async closeWindow() {
      record("close_window");
    },

    onNavigateRequested(cb): Unsubscribe {
      navigateListeners.add(cb);
      return () => navigateListeners.delete(cb);
    },
    onSettingsChanged(cb): Unsubscribe {
      settingsListeners.add(cb);
      return () => settingsListeners.delete(cb);
    },
    onHistoryHealthChanged(cb): Unsubscribe {
      healthListeners.add(cb);
      return () => healthListeners.delete(cb);
    },
    onEventRecorded(cb): Unsubscribe {
      eventListeners.add(cb);
      return () => eventListeners.delete(cb);
    },
    onCapabilitiesChanged(cb): Unsubscribe {
      capsListeners.add(cb);
      return () => capsListeners.delete(cb);
    },
    onHostsChanged(cb): Unsubscribe {
      hostsListeners.add(cb);
      return () => hostsListeners.delete(cb);
    },
    onWindowAppearanceChanged(cb): Unsubscribe {
      appearanceListeners.add(cb);
      return () => appearanceListeners.delete(cb);
    },
  };

  return transport;
}

const NO_ALERTS = { hot_process: false, thermal_serious: false };

/** Apply a settings patch the way Rust does: absent or null keeps a value. */
export function applyPatch(s: Settings, patch: SettingsPatch): Settings {
  const pick = <T extends object>(base: T, p: object | null | undefined) => {
    if (!p) return base;
    const out = { ...base };
    for (const [k, v] of Object.entries(p)) {
      if (v !== null && v !== undefined)
        (out as Record<string, unknown>)[k] = v;
    }
    return out;
  };
  const modules = { ...s.modules };
  for (const [id, mp] of Object.entries(patch.modules ?? {})) {
    const cur = modules[id as keyof typeof modules];
    if (cur && mp) modules[id as keyof typeof modules] = pick(cur, mp);
  }
  return {
    modules,
    sampling: pick(s.sampling, patch.sampling),
    history: pick(s.history, patch.history),
    units: pick(s.units, patch.units),
    general: pick(s.general, patch.general),
    onboarding: pick(s.onboarding, patch.onboarding),
    alerts: pick(s.alerts ?? NO_ALERTS, patch.alerts),
  };
}
