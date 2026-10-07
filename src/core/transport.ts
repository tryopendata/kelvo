/**
 * The seam between the frontend and Rust. Everything the UI reads or writes
 * goes through a `Transport`: `tauriTransport` in the app (generated bindings
 * plus a Tauri Channel per live subscription), `createMockTransport` in the
 * browser dev server, Vitest and Playwright (see mock-transport.ts).
 *
 * Command results keep tauri-specta's `{ status, data | error }` shape so
 * callers branch on `status` instead of catching (error-handling.md).
 */
import {
  type BatteryHour,
  type ByteCount,
  type Capabilities,
  type CapabilitiesChanged,
  type CommandError,
  commands,
  type Edition,
  type EnergyByApp,
  type Event,
  type EventRecorded,
  type ExportOutcome,
  type ExportRequest,
  events,
  type HeatmapDay,
  type HeatmapRequest,
  type HistoryGrowth,
  type HistoryHealth,
  type HistoryHealthChanged,
  type HistoryPage,
  type HistoryRequest,
  type HostId,
  type HostRecord,
  type HostsChanged,
  type LiveMsg,
  type Millis,
  type NavigateRequested,
  type NetworkAddresses,
  type NetworkByApp,
  type ProcessesAt,
  type ProcessView,
  type SensorDump,
  type SeriesSelector,
  type SettingsChanged,
  type SettingsPatch,
  type SettingsSnapshot,
  type SubscriptionInfo,
  type UpdateStatus,
  type WindowAppearance,
  type WindowAppearanceChanged,
} from "@core/generated/bindings";
import { Channel } from "@tauri-apps/api/core";
import type { ProcessSignalResult, SignalKind } from "./process-signal";

export type CommandResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: CommandError };

export type Unsubscribe = () => void;

/** What a window asks of its live channel (`subscribe_live`, D-066). */
export interface LiveOptions {
  /** Ring history to send, ms (Rust default 60 s, at most an hour). */
  backfillMs?: number;
  /**
   * The series the window draws; the channel carries only those (Rust
   * projects layouts, rows and frames). Omitted: every series.
   */
  series?: readonly SeriesSelector[];
  /** At most one frame per this many ms. Omitted: every tick. */
  minPeriodMs?: number;
}

export interface LiveSubscription {
  info: CommandResult<SubscriptionInfo>;
  /** Stop delivering messages to this callback. */
  unsubscribe: Unsubscribe;
}

export interface Transport {
  readonly kind: "tauri" | "mock";
  /** The window label (`popover`, `dashboard`, `onboarding`). */
  windowLabel(): string;

  listHosts(): Promise<HostRecord[]>;
  getHost(host: HostId): Promise<CommandResult<HostRecord>>;
  getCapabilities(host: HostId): Promise<CommandResult<Capabilities>>;
  /**
   * Start this window's live stream, replacing an earlier one. Messages
   * arrive in the D-066 order: Caps, Status, Layouts, the last two minutes
   * as Backfill, then frames; older history follows as `backfill_earlier`
   * chunks, newest first, after the first frame.
   */
  subscribeLive(
    host: HostId,
    onMsg: (msg: LiveMsg) => void,
    options?: LiveOptions
  ): Promise<LiveSubscription>;
  /**
   * Which process rows this window wants (`view`, `null` with `interested`
   * false). Replaces the window's previous view, so a window with several
   * consumers sends their union (`ProcessInterest`). `stream` is
   * `SubscriptionInfo.stream`: the interest ends with that subscription.
   */
  setProcessInterest(
    host: HostId,
    interested: boolean,
    view: ProcessView | null,
    stream: number | null
  ): Promise<CommandResult<null>>;
  queryHistory(request: HistoryRequest): Promise<CommandResult<HistoryPage>>;
  queryProcessesAt(
    host: HostId,
    tMs: Millis
  ): Promise<CommandResult<ProcessesAt | null>>;
  /**
   * Which apps moved the interface's bytes over `[fromMs, toMs)` (D-089),
   * widened to whole buckets (draw the returned `from_ms`/`to_ms`). Per
   * direction, unless `clamped`, `apps + other_apps + overhead + system`
   * equals the interface total. `remote_host` for a host other than this Mac.
   */
  queryNetworkByApp(
    host: HostId,
    fromMs: Millis,
    toMs: Millis
  ): Promise<CommandResult<NetworkByApp>>;
  /**
   * Which apps used energy over `[fromMs, toMs)` (D-093), widened to whole
   * 10 s buckets, from the last hour of process samples Rust keeps in
   * memory. `since_ms` is where counting started; `remote_host` for a host
   * other than this Mac.
   */
  queryEnergyByApp(
    host: HostId,
    fromMs: Millis,
    toMs: Millis
  ): Promise<CommandResult<EnergyByApp>>;
  /** The primary interface's addresses, read locally on each call. */
  getNetworkAddresses(host: HostId): Promise<CommandResult<NetworkAddresses>>;
  /**
   * This Mac's public address, from an outside service over HTTPS (D-093):
   * a network request, so only the Network page asks, while it shows.
   */
  getPublicIp(): Promise<CommandResult<string>>;
  /**
   * Hourly averages (`cpu.total` or `thermal.hottest`) per local day. Build
   * `days` with `heatmapDays` (heatmap-days.ts): Rust takes each day's
   * local-hour boundaries in UTC and knows no time zone. `null` hours had
   * no samples, never 0.
   */
  queryHeatmap(request: HeatmapRequest): Promise<CommandResult<HeatmapDay[]>>;
  /**
   * The hourly battery bars, one per local hour between consecutive
   * `hourStartsMs` (build them with `batteryHourStarts`), through now. `null`
   * charge is an hour without samples, never 0.
   */
  batteryHours(
    host: HostId,
    hourStartsMs: Millis[]
  ): Promise<CommandResult<BatteryHour[]>>;
  /**
   * Detector and alert events (v1.2) with `fromMs <= ts_ms < toMs`, oldest
   * first. A live view adds new ones from `onEventRecorded` instead of
   * refetching.
   */
  queryEvents(
    host: HostId,
    fromMs: Millis,
    toMs: Millis
  ): Promise<CommandResult<Event[]>>;
  /**
   * Rust opens a save dialog, then streams the range to the chosen file as
   * CSV. Resolves `{ kind: "cancelled" }` when the dialog is dismissed;
   * `export` errors mean the file could not be written.
   */
  exportCsv(request: ExportRequest): Promise<CommandResult<ExportOutcome>>;

  getSettings(): Promise<SettingsSnapshot>;
  updateSettings(
    patch: SettingsPatch
  ): Promise<CommandResult<SettingsSnapshot>>;
  historySize(host: HostId): Promise<CommandResult<ByteCount>>;
  historyGrowth(host: HostId): Promise<CommandResult<HistoryGrowth | null>>;
  clearHistory(host: HostId): Promise<CommandResult<ByteCount>>;
  /** Low-disk pause and size-limit trim (D-057, D-059); shared by every host. */
  historyHealth(host: HostId): Promise<CommandResult<HistoryHealth>>;
  /**
   * Move the history file aside (kept as `history-reset-<ms>.sqlite`) and
   * start an empty one (D-064). `store_busy` while another Kelvo process
   * holds it. Returns the new size on disk.
   */
  resetHistory(): Promise<CommandResult<ByteCount>>;
  setPaused(paused: boolean): Promise<void>;
  openDashboard(route: string | null): Promise<CommandResult<null>>;
  sensorDump(host: HostId): Promise<CommandResult<SensorDump>>;
  /**
   * Quit or Force Quit one process (D-029). `startTimeUs` is the start time
   * the row showed; Rust answers `pid_reused` when the PID now belongs to a
   * different process.
   */
  processSignal(
    host: HostId,
    pid: number,
    startTimeUs: number,
    kind: SignalKind
  ): Promise<ProcessSignalResult>;
  /** What this build can do (D-065); fixed for the app run. */
  getEdition(): Promise<Edition>;
  checkForUpdates(): Promise<UpdateStatus>;
  getWindowAppearance(): Promise<WindowAppearance>;
  /** Open an http(s) URL in the default browser (sensor dump GitHub issue). */
  openUrl(url: string): Promise<void>;
  /** Close this window (onboarding Skip and Done). */
  closeWindow(): Promise<void>;

  /**
   * The dashboard window only: Rust asks an open dashboard to show a route
   * (`open_dashboard` with a route while the window exists).
   */
  onNavigateRequested(cb: (event: NavigateRequested) => void): Unsubscribe;
  onSettingsChanged(cb: (event: SettingsChanged) => void): Unsubscribe;
  onCapabilitiesChanged(cb: (event: CapabilitiesChanged) => void): Unsubscribe;
  /** Every host record, local first, when one changes (`chip_known`). */
  onHostsChanged(cb: (event: HostsChanged) => void): Unsubscribe;
  onHistoryHealthChanged(
    cb: (event: HistoryHealthChanged) => void
  ): Unsubscribe;
  /**
   * A detector or alert fired on a host. Sent once the event is queued with
   * a commit request, not once committed: a `queryEvents` started right
   * after may still miss it (D-083).
   */
  onEventRecorded(cb: (event: EventRecorded) => void): Unsubscribe;
  onWindowAppearanceChanged(
    cb: (event: WindowAppearanceChanged) => void
  ): Unsubscribe;
}

/** Turn Tauri's async unlisten into a synchronous one for effect cleanups. */
function syncUnlisten(pending: Promise<() => void>): Unsubscribe {
  let unlisten: (() => void) | null = null;
  let cancelled = false;
  pending.then(
    (fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    },
    (err: unknown) => console.error("[transport] listen failed", err)
  );
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

/** True inside a Tauri webview. */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * The app transport: generated commands and events, one Channel per live
 * subscription. Rust keys the channel by window label and host and owns when
 * it stops (hidden, occluded, display asleep); a resubscribe from the same
 * window replaces it. There is no unsubscribe command: dropping the callback
 * here only stops delivery, and closing the window drops the channel.
 */
export function createTauriTransport(label: string): Transport {
  return {
    kind: "tauri",
    windowLabel: () => label,
    listHosts: () => commands.listHosts(),
    getHost: (host) => commands.getHost(host),
    getCapabilities: (host) => commands.getCapabilities(host),
    async subscribeLive(host, onMsg, options = {}) {
      let active = true;
      const channel = new Channel<LiveMsg>();
      channel.onmessage = (msg) => {
        if (active) onMsg(msg);
      };
      const info = await commands.subscribeLive(
        host,
        channel,
        options.backfillMs ?? null,
        options.series ? [...options.series] : null,
        options.minPeriodMs ?? null
      );
      return {
        info,
        unsubscribe: () => {
          active = false;
        },
      };
    },
    setProcessInterest: (host, interested, view, stream) =>
      commands.setProcessInterest(host, interested, view, stream),
    queryHistory: (request) => commands.queryHistory(request),
    queryProcessesAt: (host, tMs) => commands.queryProcessesAt(host, tMs),
    queryNetworkByApp: (host, fromMs, toMs) =>
      commands.queryNetworkByApp(host, fromMs, toMs),
    queryEnergyByApp: (host, fromMs, toMs) =>
      commands.queryEnergyByApp(host, fromMs, toMs),
    getNetworkAddresses: (host) => commands.getNetworkAddresses(host),
    getPublicIp: () => commands.getPublicIp(),
    queryEvents: (host, fromMs, toMs) =>
      commands.queryEvents(host, fromMs, toMs),
    queryHeatmap: (request) => commands.queryHeatmap(request),
    batteryHours: (host, hourStartsMs) =>
      commands.batteryHours(host, hourStartsMs),
    exportCsv: (request) => commands.exportCsv(request),
    getSettings: () => commands.getSettings(),
    updateSettings: (patch) => commands.updateSettings(patch),
    historySize: (host) => commands.historySize(host),
    historyGrowth: (host) => commands.historyGrowth(host),
    clearHistory: (host) => commands.clearHistory(host),
    historyHealth: (host) => commands.historyHealth(host),
    resetHistory: () => commands.resetHistory(),
    setPaused: (paused) => commands.setPaused(paused),
    openDashboard: (route) => commands.openDashboard(route),
    sensorDump: (host) => commands.sensorDump(host),
    processSignal: (host, pid, startTimeUs, kind) =>
      commands.processSignal(host, pid, startTimeUs, kind),
    getEdition: () => commands.getEdition(),
    checkForUpdates: () => commands.checkForUpdates(),
    getWindowAppearance: () => commands.getWindowAppearance(),
    async openUrl(url) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(url);
    },
    async closeWindow() {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().close();
    },
    onNavigateRequested: (cb) =>
      syncUnlisten(events.navigateRequested.listen((e) => cb(e.payload))),
    onSettingsChanged: (cb) =>
      syncUnlisten(events.settingsChanged.listen((e) => cb(e.payload))),
    onCapabilitiesChanged: (cb) =>
      syncUnlisten(events.capabilitiesChanged.listen((e) => cb(e.payload))),
    onHostsChanged: (cb) =>
      syncUnlisten(events.hostsChanged.listen((e) => cb(e.payload))),
    onHistoryHealthChanged: (cb) =>
      syncUnlisten(events.historyHealthChanged.listen((e) => cb(e.payload))),
    onEventRecorded: (cb) =>
      syncUnlisten(events.eventRecorded.listen((e) => cb(e.payload))),
    onWindowAppearanceChanged: (cb) =>
      syncUnlisten(events.windowAppearanceChanged.listen((e) => cb(e.payload))),
  };
}

/** Raised by query functions so TanStack Query sees a failed command. */
export class CommandFailure extends Error {
  readonly error: CommandError;
  constructor(error: CommandError) {
    super("message" in error ? `${error.kind}: ${error.message}` : error.kind);
    this.name = "CommandFailure";
    this.error = error;
  }
}

/** Unwrap a command result for a query function: data, or a thrown failure. */
export async function unwrap<T>(result: Promise<CommandResult<T>>): Promise<T> {
  const r = await result;
  if (r.status === "error") throw new CommandFailure(r.error);
  return r.data;
}
