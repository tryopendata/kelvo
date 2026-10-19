/**
 * The seam between the frontend and Rust. Everything the UI reads or writes
 * goes through a `Transport`: `tauriTransport` in the app (generated bindings
 * plus a Tauri Channel per live subscription), `createMockTransport` in the
 * browser dev server, Vitest and Playwright (see mock-transport.ts).
 *
 * Command results keep tauri-specta's `{ status, data | error }` shape so
 * callers branch on `status` instead of catching (error-handling.md).
 */
import type {
  BatteryHour,
  ByteCount,
  Capabilities,
  CapabilitiesChanged,
  CommandError,
  DiskIoDevices,
  DiskIoPage,
  DiskIoRequest,
  DockerComparisonPage,
  DockerComparisonRequest,
  DockerContainer,
  DockerContainerAction,
  DockerEngine,
  DockerEventsPage,
  DockerEventsRequest,
  DockerHistoryPage,
  DockerHistoryRequest,
  DockerLogBatch,
  DockerLogsRequest,
  DockerRetainedPage,
  DockerRetainedRequest,
  DockerSettings,
  DockerSnapshot,
  DockerStorageOperation,
  DockerStorageReview,
  DockerStorageSelection,
  DockerStorageView,
  Edition,
  Event,
  EventRecorded,
  ExportOutcome,
  ExportRequest,
  HeatmapDay,
  HeatmapRequest,
  HistoryGrowth,
  HistoryHealth,
  HistoryHealthChanged,
  HistoryPage,
  HistoryRequest,
  HostId,
  HostRecord,
  HostsChanged,
  LiveMsg,
  Millis,
  NavigateRequested,
  NetworkAddresses,
  NetworkByApp,
  ProcessesAt,
  ProcessView,
  RemoteHostConfig,
  RemoteHostPreparation,
  RemoteHostRecord,
  RemoteSshConfigStatus,
  RemoteSshHostTrust,
  SensorDump,
  SeriesSelector,
  SeriesStats,
  SettingsChanged,
  SettingsPatch,
  SettingsSnapshot,
  SpaceFile,
  SpaceReport,
  SpaceScanStatus,
  SpaceSubtree,
  SpaceTarget,
  SpaceVolume,
  SubscriptionInfo,
  UpdateStatus,
  UsageByApp,
  UsageKey,
  WindowAppearance,
  WindowAppearanceChanged,
} from "@core/generated/bindings";
import type { ProcessSignalResult, SignalKind } from "../process-signal";

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

  dockerComparison(
    request: DockerComparisonRequest
  ): Promise<CommandResult<DockerComparisonPage>>;
  dockerContainerAction(
    engineId: string,
    containerId: string,
    action: DockerContainerAction
  ): Promise<CommandResult<DockerStorageOperation>>;
  dockerStorageScan(force: boolean): Promise<CommandResult<DockerStorageView>>;
  dockerStoragePrepare(
    selections: DockerStorageSelection[]
  ): Promise<CommandResult<DockerStorageReview>>;
  dockerStorageExecute(
    token: string,
    confirmVolumes: boolean
  ): Promise<CommandResult<DockerStorageOperation>>;
  dockerStorageOperation(
    operationId: string
  ): Promise<CommandResult<DockerStorageOperation>>;
  dockerRetained(
    request: DockerRetainedRequest
  ): Promise<CommandResult<DockerRetainedPage>>;
  dockerDiscover(): Promise<CommandResult<DockerEngine[]>>;
  dockerConfigure(
    settings: DockerSettings
  ): Promise<CommandResult<DockerSnapshot>>;
  dockerSnapshot(): Promise<CommandResult<DockerSnapshot>>;
  dockerSubscribe(
    onSnapshot: (snapshot: DockerSnapshot) => void
  ): Promise<{ info: CommandResult<number>; unsubscribe: Unsubscribe }>;
  dockerDetail(
    engineId: string,
    containerId: string
  ): Promise<CommandResult<DockerContainer>>;
  dockerHistory(
    request: DockerHistoryRequest
  ): Promise<CommandResult<DockerHistoryPage>>;
  dockerEvents(
    request: DockerEventsRequest
  ): Promise<CommandResult<DockerEventsPage>>;
  dockerLogs(
    request: DockerLogsRequest,
    onBatch: (batch: DockerLogBatch) => void
  ): Promise<{ info: CommandResult<number>; unsubscribe: Unsubscribe }>;
  dockerResume(): Promise<CommandResult<DockerSnapshot>>;
  listHosts(): Promise<HostRecord[]>;
  listRemoteHosts(): Promise<RemoteHostRecord[]>;
  remoteSshConfigStatus(): Promise<CommandResult<RemoteSshConfigStatus>>;
  inspectRemoteSshHost(
    alias: string
  ): Promise<CommandResult<RemoteSshHostTrust>>;
  acceptRemoteSshHost(token: string): Promise<CommandResult<null>>;
  prepareRemoteHost(
    lan: string | null,
    tailscale: string | null
  ): Promise<CommandResult<RemoteHostPreparation>>;
  addRemoteHost(
    config: RemoteHostConfig,
    install: boolean,
    showMenuBar: boolean,
    setupId: string
  ): Promise<CommandResult<RemoteHostRecord>>;
  cancelRemoteSetup(setupId: string): Promise<CommandResult<null>>;
  updateRemoteHost(
    config: RemoteHostConfig
  ): Promise<CommandResult<RemoteHostRecord>>;
  removeRemoteHost(
    id: string,
    deleteMirror: boolean,
    stopSharing: boolean
  ): Promise<CommandResult<string | null>>;
  retryRemoteHost(id: string): Promise<CommandResult<null>>;
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
  queryDiskIo(request: DiskIoRequest): Promise<CommandResult<DiskIoPage>>;
  listDiskIoDevices(host: HostId): Promise<CommandResult<DiskIoDevices>>;
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
   * Which apps used CPU, GPU, memory, disk and energy over `[fromMs, toMs)`
   * (D-099), widened to whole 10 s buckets, from the last hour of process
   * samples Rust keeps in memory: the largest `limit` by `by`, with what the
   * host measured beyond them in `other`. `remote_host` for a host other than
   * this Mac.
   */
  queryUsageByApp(
    host: HostId,
    fromMs: Millis,
    toMs: Millis,
    by: UsageKey,
    limit: number
  ): Promise<CommandResult<UsageByApp>>;
  /**
   * Unlabelled metrics over `[fromMs, toMs)`, widened to whole buckets and cut
   * at now: each one's average, largest sample and integral over the time
   * measured.
   */
  querySeriesStats(
    host: HostId,
    metrics: string[],
    fromMs: Millis,
    toMs: Millis
  ): Promise<CommandResult<SeriesStats>>;
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

  /** Volumes this Mac can scan for a space report (local only). */
  diskSpaceVolumes(): Promise<CommandResult<SpaceVolume[]>>;
  /**
   * Starts scanning a volume; `include` lists protected locations (by key)
   * the user chose to enter. Fails while another scan runs.
   */
  diskSpaceStart(
    volume: string,
    include: string[]
  ): Promise<CommandResult<SpaceScanStatus>>;
  /** Progress of the running scan, or how the last one ended. */
  diskSpaceStatus(): Promise<SpaceScanStatus>;
  diskSpaceCancel(): Promise<void>;
  /** The finished report, kept while the dashboard window exists. */
  diskSpaceReport(): Promise<SpaceReport | null>;
  /** A folder's breakdown (`node: null` is the volume root), during or after a scan. */
  diskSpaceSubtree(
    report: number,
    node: number | null
  ): Promise<CommandResult<SpaceSubtree>>;
  diskSpaceLargestFiles(report: number): Promise<CommandResult<SpaceFile[]>>;
  diskSpaceReveal(
    report: number,
    target: SpaceTarget
  ): Promise<CommandResult<null>>;
  /** An entry's path as Finder shows it ("/Users/…" on the boot volume). */
  diskSpacePath(
    report: number,
    target: SpaceTarget
  ): Promise<CommandResult<string>>;
  getUpdateStatus(): Promise<UpdateStatus>;
  checkForUpdates(): Promise<CommandResult<UpdateStatus>>;
  installUpdate(version: string): Promise<CommandResult<null>>;
  onUpdateStatusChanged(cb: (status: UpdateStatus) => void): Unsubscribe;
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
