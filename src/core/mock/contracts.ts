import type {
  ChartWindow,
  Event,
  HistoryGrowth,
  HistoryHealth,
  LiveMsg,
  ProcessView,
  SeriesKey,
  UpdateStatus,
} from "@core/generated/bindings";
import type { Transport } from "../transport";
import type { ScenarioFlags, ScenarioName } from "./fixtures";
export interface MockTransportOptions {
  updateStatus?: UpdateStatus;
  updateFails?: boolean;
  /** A bounded setup delay for cancellation tests. */
  remoteSetupDelayMs?: number;
  /** Connected peers predating durable usage reject only those queries. */
  remoteUsageHistorySupported?: boolean;
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
   * The host's side of a controller's session ending (D-117): its space
   * scan stops and its report goes. A remote host's peer gets this when the
   * remote leaves `connected`.
   */
  endSpaceSession(): void;
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
export interface HistoryRow {
  ts: number;
  /** The engine's tick when it was sampled: 2 s in the background (D-094). */
  interval: number;
  timeline: number;
  values: (number | null)[];
  /** How long each series' sample stays current (D-090), generator order. */
  holds: number[];
}
export interface LiveSub {
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
