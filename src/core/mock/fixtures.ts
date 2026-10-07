/**
 * Typed fixtures for the mock transport: sample data (machine header,
 * processes) and plan 6.4's settings defaults.
 * Typed with the generated bindings, so a Rust type change breaks them at
 * compile time.
 */
import type {
  Capabilities,
  ClusterInfo,
  HistoryUnavailableReason,
  HostInfo,
  HostRecord,
  LiveProcess,
  ModuleCap,
  Settings,
  SignalRefusal,
} from "@core/generated/bindings";

export type ScenarioName =
  | "default"
  | "sleep-gap"
  | "unknown-chip"
  | "no-battery"
  | "no-fans"
  | "paused"
  | "stale"
  | "history-unavailable"
  | "low-disk"
  | "history-trimmed"
  | "history-locked"
  | "history-corrupt"
  | "history-too-new"
  | "appstore"
  | "clock-step"
  | "cpu-power-uncalibrated"
  | "cpu-power-seeded"
  | "cpu-power-calibrated"
  | "wide-layout"
  | "hidden-resume"
  | "no-process-network"
  | "no-process-gpu"
  | "low-power-mode"
  | "performance-mode"
  | "vpn";

export const SCENARIOS: readonly ScenarioName[] = [
  "default",
  "sleep-gap",
  "unknown-chip",
  "no-battery",
  "no-fans",
  "paused",
  "stale",
  "history-unavailable",
  "low-disk",
  "history-trimmed",
  "history-locked",
  "history-corrupt",
  "history-too-new",
  "appstore",
  "clock-step",
  "cpu-power-uncalibrated",
  "cpu-power-seeded",
  "cpu-power-calibrated",
  "wide-layout",
  "hidden-resume",
  "no-process-network",
  "no-process-gpu",
  "low-power-mode",
  "performance-mode",
  "vpn",
];

/** The schema version the mock's "newer Kelvo" wrote, and the one it reads. */
export const MOCK_SCHEMA = { found: 4, supported: 2 } as const;

/** Series in the `wide-layout` scenario: a full Mac's layout (D-066). */
export const WIDE_LAYOUT_SERIES = 170;

export interface ScenarioFlags {
  sleepGap: boolean;
  unknownChip: boolean;
  noBattery: boolean;
  noFans: boolean;
  paused: boolean;
  stale: boolean;
  /** The store failed to open: history commands answer `history_unavailable`. */
  historyUnavailable: boolean;
  /** The disk is almost full: 10 s history is paused (D-057). */
  lowDisk: boolean;
  /** The size limit trimmed history 20 days back (D-057, D-059). */
  historyTrimmed: boolean;
  /**
   * Why history is unavailable (D-064): `history-locked`, `history-corrupt`
   * and `history-too-new` set it (and `historyUnavailable`); plain
   * `history-unavailable` leaves it null (reason unknown).
   */
  historyReason: HistoryUnavailableReason | null;
  /** The App Store edition: no Quit or Force Quit (D-065). */
  appstore: boolean;
  /** The auto-tick steps the wall clock back five minutes after ten ticks. */
  clockStep: boolean;
  /**
   * `power.cpu_source` (D-054, D-065); null leaves the series out (PMP).
   * Set, the chip is an M3 Max on SMC power: no E-cluster power series.
   */
  cpuPowerSource: number | null;
  /** Pad the layout to `WIDE_LAYOUT_SERIES` series (the perf gate). */
  wideLayout: boolean;
  /**
   * After ten ticks the window is hidden for half an hour of samples, then
   * shown: the channel resumes with the missed span (the perf gate).
   */
  hiddenResume: boolean;
  /**
   * `Capabilities.process_network` is false: NetworkStatistics did not load
   * (D-081), so the network process columns hide and the Overview Network
   * card keeps its interface list. The `appstore` scenario implies it.
   */
  noProcessNetwork: boolean;
  /**
   * `Capabilities.process_gpu` is false: the GPU's registry clients could
   * not be read (D-085), so the GPU process columns hide and the Overview
   * GPU card keeps its 60 s chart. The `appstore` scenario implies it.
   */
  noProcessGpu: boolean;
  /**
   * macOS Low Power Mode is on: Performance mode engages without a settings
   * write (D-088). `setLowPowerMode` changes it at run time.
   */
  lowPowerMode: boolean;
  /** Settings start with Performance mode on (the perf gate's comparison). */
  performanceMode: boolean;
  /**
   * A full-tunnel VPN on a Mac with Wi-Fi and a dock's Ethernet: both are
   * reported (en0, en7), but the default route is on the tunnel, which is
   * not, so the status's `primary_iface` is null (D-092) while the totals
   * cover both interfaces.
   */
  vpn: boolean;
}

export function scenarioFlags(names: readonly ScenarioName[]): ScenarioFlags {
  const has = (n: ScenarioName) => names.includes(n);
  const historyReason: HistoryUnavailableReason | null = has("history-locked")
    ? { kind: "locked" }
    : has("history-corrupt")
      ? { kind: "corrupt", message: "database disk image is malformed" }
      : has("history-too-new")
        ? { kind: "too_new", ...MOCK_SCHEMA }
        : null;
  return {
    sleepGap: has("sleep-gap"),
    unknownChip: has("unknown-chip"),
    noBattery: has("no-battery"),
    noFans: has("no-fans"),
    paused: has("paused"),
    stale: has("stale"),
    historyUnavailable: has("history-unavailable") || historyReason !== null,
    lowDisk: has("low-disk"),
    historyTrimmed: has("history-trimmed"),
    historyReason,
    appstore: has("appstore"),
    clockStep: has("clock-step"),
    cpuPowerSource: has("cpu-power-uncalibrated")
      ? 1
      : has("cpu-power-seeded")
        ? 3
        : has("cpu-power-calibrated")
          ? 2
          : null,
    wideLayout: has("wide-layout"),
    hiddenResume: has("hidden-resume"),
    noProcessNetwork: has("no-process-network") || has("appstore"),
    noProcessGpu: has("no-process-gpu") || has("appstore"),
    lowPowerMode: has("low-power-mode"),
    performanceMode: has("performance-mode"),
    vpn: has("vpn"),
  };
}

export const MOCK_HOST_ID = "6f1c2a4e-8b7d-4c1e-9a3f-2d5e7b9c0a11";

/** DVFS tables in MHz, ascending; the last entry is the cluster maximum. */
export const P_DVFS = [1260, 1924, 2420, 3204, 3864, 4512];
export const E_DVFS = [1020, 1500, 2160, 2892];

export const P_CORES = Array.from({ length: 10 }, (_, i) => `P${i}`);
export const E_CORES = Array.from({ length: 4 }, (_, i) => `E${i}`);

export const TOPOLOGY: ClusterInfo[] = [
  { name: "P0", kind: "performance", cores: P_CORES, dvfs_mhz: P_DVFS },
  { name: "E0", kind: "efficiency", cores: E_CORES, dvfs_mhz: E_DVFS },
];

/** 24 GiB: the marketing "24 GB" (plan 4.9). */
export const MEM_TOTAL_BYTES = 24 * 2 ** 30;

/** Sample uptime: "up 3d 4h" (popover), "3d 4h 12m" (machine header). */
export const UPTIME_MS = ((3 * 24 + 4) * 60 + 12) * 60_000;

export function hostInfo(flags: ScenarioFlags, nowMs: number): HostInfo {
  return {
    os: "mac_os",
    os_version: "27.0.1",
    model: flags.unknownChip ? "Mac17,4" : "Mac16,8",
    chip: flags.unknownChip ? "Apple M5 Pro" : "Apple M4 Pro",
    chip_known: !flags.unknownChip,
    cpu_topology: TOPOLOGY,
    mem_total_bytes: MEM_TOTAL_BYTES,
    boot_time_ms: nowMs - UPTIME_MS,
    // The GPU DVFS table without its off state, topping out at the mock's
    // highest `gpu.residency` state (D-092).
    gpu_dvfs_mhz: [338, 720, 1098, 1398, 1578],
    // The boot container's mounts, as `disk.total{vol}` lists them.
    boot_mounts: ["/", "/System/Volumes/Data"],
  };
}

export function hostRecord(flags: ScenarioFlags, nowMs: number): HostRecord {
  return {
    id: MOCK_HOST_ID,
    is_local: true,
    display_name: flags.noBattery ? "Mac mini" : "MacBook Pro",
    info: hostInfo(flags, nowMs),
  };
}

function available(series: number): ModuleCap {
  return { available: { series } };
}

export function capabilities(
  flags: ScenarioFlags,
  seriesPerModule: Record<string, number>,
  revision = 1
): Capabilities {
  const n = (m: string) => seriesPerModule[m] ?? 0;
  return {
    revision,
    modules: {
      cpu: available(n("cpu")),
      gpu: available(n("gpu")),
      memory: available(n("memory")),
      power: available(n("power")),
      sensors: flags.unknownChip
        ? { unsupported: "unknown_chip" }
        : available(n("sensors")),
      network: available(n("network")),
      disk: available(n("disk")),
      battery: flags.noBattery ? "not_present" : available(n("battery")),
    },
    process_network: !flags.noProcessNetwork,
    process_gpu: !flags.noProcessGpu,
  };
}

/** Plan 6.4 defaults, with every module on so the mock shows all of them. */
export function defaultSettings(flags: ScenarioFlags): Settings {
  return {
    modules: {
      cpu: { enabled: true, menu_bar: "in_combined" },
      gpu: { enabled: true, menu_bar: "in_combined" },
      memory: { enabled: true, menu_bar: "in_combined" },
      power: { enabled: true, menu_bar: "temp_in_combined" },
      network: { enabled: true, menu_bar: "hidden" },
      disk: { enabled: true, menu_bar: "hidden" },
      battery: { enabled: !flags.noBattery, menu_bar: "hidden" },
    },
    // The mock shows "Sampling every 1s" while on battery, so it has the
    // battery slowdown off.
    sampling: {
      interval_ms: 1000,
      slow_on_battery: false,
      performance_mode: flags.performanceMode,
    },
    history: { retention_days: 30, size_limit_mb: 150, network_history: true },
    units: {
      // °C, not the app's °F default (D-093): the sample figures are in °C.
      temperature: "celsius",
      network: "bytes_per_sec",
      memory: "decimal",
    },
    general: {
      launch_at_login: true,
      show_in_dock: false,
      appearance: "system",
      check_updates: true,
      chart_window: "15m",
    },
    onboarding: { completed: true },
    alerts: { hot_process: false, thermal_serious: false },
  };
}

/** The mock app's own pid: the Kelvo row. */
export const MOCK_SELF_PID = 2010;

/** The CPU page's "Top processes", plus the Overview's memory and disk figures. */
export const PROCESSES: LiveProcess[] = [
  proc(2214, "Xcode", 86.8, 3.8e9, 74, 312, 41.2, 142e6, "me"),
  proc(0, "kernel_task", 43.4, 1.1e9, 612, 1904, 18.8, 4e6, "root"),
  proc(391, "WindowServer", 33.6, 0.6e9, 22, 840, 6.1, 1e6, "_windowserver"),
  proc(1840, "Safari", 25.2, 1.9e9, 41, 226, 9.6, 6e6, "me"),
  proc(5531, "node", 15.4, 0.84e9, 12, 64, 2.4, 3e6, "me"),
  proc(1712, "Figma", 12.6, 1.2e9, 38, 118, 7.4, 0.5e6, "me"),
  proc(988, "com.docker.backend", 9.8, 2.6e9, 29, 402, 3.1, 51e6, "me"),
  proc(466, "mds_stores", 4.2, 0.12e9, 7, 12, 0.8, 18e6, "root"),
];

/**
 * Quiet background processes, so the Processes page has a realistic count
 * and a scrolling table. Every figure stays below the `PROCESSES`
 * rows, so the top-N lists are unchanged. Includes launchd
 * (refused) and root-owned ones (EPERM in `process_signal`).
 */
export const BACKGROUND_PROCESSES: LiveProcess[] = [
  proc(1, "launchd", 0.4, 0.02e9, 4, 9, 0.1, 0, "root"),
  proc(97, "logd", 0.6, 0.03e9, 6, 22, 0.1, 0.1e6, "root"),
  proc(102, "configd", 0.2, 0.01e9, 7, 4, 0, 0, "root"),
  proc(118, "syslogd", 0.1, 0.01e9, 5, 3, 0, 0.05e6, "root"),
  proc(131, "powerd", 0.1, 0.008e9, 3, 2, 0, 0, "root"),
  proc(146, "bluetoothd", 0.5, 0.02e9, 9, 18, 0.1, 0, "root"),
  proc(162, "coreaudiod", 0.8, 0.03e9, 11, 40, 0.2, 0, "_coreaudiod"),
  proc(184, "airportd", 0.3, 0.02e9, 8, 11, 0.1, 0, "root"),
  proc(431, "mds", 0.7, 0.05e9, 9, 16, 0.1, 0.2e6, "root"),
  proc(612, "loginwindow", 0.1, 0.06e9, 4, 2, 0, 0, "me"),
  proc(640, "Dock", 0.4, 0.09e9, 6, 8, 0.1, 0, "me"),
  proc(651, "SystemUIServer", 0.2, 0.05e9, 5, 4, 0, 0, "me"),
  proc(655, "Finder", 0.3, 0.09e9, 9, 6, 0.1, 0.05e6, "me"),
  proc(702, "ControlCenter", 0.5, 0.07e9, 8, 14, 0.1, 0, "me"),
  proc(744, "Spotlight", 0.2, 0.06e9, 7, 3, 0, 0, "me"),
  proc(781, "NotificationCenter", 0.1, 0.05e9, 5, 2, 0, 0, "me"),
  proc(812, "cloudd", 0.6, 0.04e9, 10, 12, 0.1, 0.25e6, "me"),
  proc(845, "photoanalysisd", 0.9, 0.08e9, 12, 20, 0.3, 0.1e6, "me"),
  proc(903, "Music", 0.4, 0.09e9, 18, 30, 0.2, 0, "me"),
  proc(1203, "Terminal", 0.3, 0.07e9, 7, 5, 0.1, 0, "me"),
  proc(1288, "zsh", 0, 0.004e9, 1, 0, 0, 0, "me"),
  proc(1977, "Safari Web Content", 0.9, 0.09e9, 14, 38, 0.3, 0.05e6, "me"),
  proc(2010, "Kelvo", 0.3, 0.06e9, 9, 6, 0.1, 0, "me"),
  proc(2240, "SourceKitService", 0.8, 0.09e9, 6, 4, 0.2, 0.1e6, "me"),
];

/** About how many processes a working Mac runs; the perf gate's load (plan 7). */
export const MOCK_PROCESS_COUNT = 800;

const IDLE_NAMES = [
  "com.apple.WebKit.WebContent",
  "XPCService",
  "mdworker_shared",
  "distnoted",
  "cfprefsd",
  "trustd",
  "secd",
  "usernoted",
  "nsurlsessiond",
  "Code Helper (Renderer)",
  "Google Chrome Helper (Renderer)",
  "Slack Helper",
  "containermanagerd",
  "lsd",
  "AMPDeviceDiscoveryAgent",
  "CategoriesService",
];
const IDLE_USERS = ["me", "me", "root", "_spotlight"];

/**
 * Near-idle processes that fill the list out to `MOCK_PROCESS_COUNT`, so
 * every process path (the store, the Overview top-5 lists, the Processes
 * table) runs at a realistic size. Deterministic, and every figure stays
 * below the background rows', so no top-N list changes.
 */
export const IDLE_PROCESSES: LiveProcess[] = Array.from(
  {
    length: MOCK_PROCESS_COUNT - PROCESSES.length - BACKGROUND_PROCESSES.length,
  },
  (_, i) =>
    proc(
      3000 + i * 7,
      IDLE_NAMES[i % IDLE_NAMES.length] as string,
      (i % 10) / 100,
      (5 + (i % 40)) * 1e6,
      1 + (i % 9),
      i % 3,
      0,
      0,
      IDLE_USERS[i % IDLE_USERS.length] as string
    )
);

/**
 * `SignalRefusal::of` (`process_signal/mod.rs`) for a mock row, with the
 * Kelvo row as the mock app's only own process.
 */
function refusal(pid: number, name: string): SignalRefusal | null {
  if (pid <= 0) return "kernel_task";
  if (pid === 1) return "launchd";
  if (pid === MOCK_SELF_PID) return "kelvo";
  switch (name) {
    case "kernel_task":
      return "kernel_task";
    case "launchd":
      return "launchd";
    case "WindowServer":
      return "window_server";
    case "loginwindow":
      return "login_window";
    default:
      return null;
  }
}

function proc(
  pid: number,
  name: string,
  cpu: number,
  mem: number,
  threads: number,
  wakeups: number,
  energy: number,
  diskBps: number,
  user: string
): LiveProcess {
  return {
    pid,
    // A fixed start time per process; pid plus start time is the identity.
    start_time_us: 1_759_500_000_000_000 + pid * 1_000_000,
    name,
    cpu_pct: cpu,
    mem_bytes: mem,
    compressed_bytes: Math.round(mem * 0.08),
    threads,
    idle_wakeups_per_s: wakeups,
    energy,
    disk_read_bps: Math.round(diskBps * 0.7),
    disk_write_bps: Math.round(diskBps * 0.3),
    net_rx_bps: null,
    net_tx_bps: null,
    gpu_pct: null,
    user,
    refusal: refusal(pid, name),
  };
}

/**
 * Network receive and send rates by pid, bytes/s: the Overview Network card's
 * top 5 (Safari 22.1, Docker 9.6, node 4.2, Xcode 1.8, Figma 0.7 MB/s in
 * total) and a few quiet ones below them. Every other process gets 0, as
 * Rust reports a process with no flows in a measured batch (D-081).
 */
export const NET_RATES: ReadonlyMap<number, readonly [number, number]> =
  new Map([
    [1840, [21.4e6, 0.7e6]],
    [988, [8.8e6, 0.8e6]],
    [5531, [3.0e6, 1.2e6]],
    [2214, [1.2e6, 0.6e6]],
    [1712, [0.5e6, 0.2e6]],
    [812, [0.12e6, 0.03e6]],
    [1977, [0.06e6, 0.01e6]],
    [903, [0.04e6, 0.002e6]],
  ]);

/** `rows` with measured network rates, as a batch for a view with `network`. */
export function withNetRates(rows: readonly LiveProcess[]): LiveProcess[] {
  return rows.map((p) => {
    const [rx, tx] = NET_RATES.get(p.pid) ?? [0, 0];
    return { ...p, net_rx_bps: rx, net_tx_bps: tx };
  });
}

/**
 * Percent of the whole GPU by pid: the Overview GPU card's top 5 (WindowServer
 * 14.2, Figma 9.8, Safari 5.1, Xcode 3.4, Docker 0.6) and a few quiet ones
 * below them. Every other process gets 0, as Rust reports a process without
 * GPU time in a measured batch (D-085).
 */
export const GPU_PCT: ReadonlyMap<number, number> = new Map([
  [391, 14.2],
  [1712, 9.8],
  [1840, 5.1],
  [2214, 3.4],
  [988, 0.6],
  [1977, 0.4],
  [2010, 0.3],
  [640, 0.1],
]);

/** `rows` with measured GPU time, as a batch for a view with `gpu`. */
export function withGpuPct(rows: readonly LiveProcess[]): LiveProcess[] {
  return rows.map((p) => ({ ...p, gpu_pct: GPU_PCT.get(p.pid) ?? 0 }));
}
