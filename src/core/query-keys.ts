/**
 * Centralised TanStack Query keys (architecture.md "History: TanStack Query",
 * infra 2). Never build a key inline: invalidation relies on these prefixes.
 * Every key starts with its family and the host, so `historyKeys.host(id)`
 * drops one host's history and `historyKeys.all` every host's.
 */
import type {
  HeatmapMetric,
  HostId,
  Module,
  Settings,
  Tier,
  UsageKey,
} from "@core/generated/bindings";

/**
 * A history window. `endMs: null` follows now (Live); a number is a window
 * the user stepped back to. `bucketMs` is the width the range is read at:
 * 7d and 30d pick it from the plot width, and a resize across a step of
 * that ladder reads again.
 */
export interface RangeSpec {
  span: "1h" | "6h" | "24h" | "7d" | "30d";
  endMs: number | null;
  bucketMs: number;
}

export type TierKey = Tier | "auto";

export const historyKeys = {
  all: ["history"] as const,
  host: (hostId: HostId) => [...historyKeys.all, hostId] as const,
  range: (hostId: HostId, module: Module, range: RangeSpec, tier: TierKey) =>
    [...historyKeys.host(hostId), module, range, tier] as const,
  /** 24 h maxima that scale Overview bars (GPU power, disk rates). */
  maxima: (hostId: HostId, module: Module, metric: string) =>
    [...historyKeys.host(hostId), module, "max24h", metric] as const,
  /** Hourly battery bars, by the end of the current local hour. */
  batteryHours: (hostId: HostId, endHourMs: number) =>
    [...historyKeys.host(hostId), "battery", "hours", endHourMs] as const,
  /**
   * Stored processes for one Timeline bucket. 10 s, 1 min and 15 min buckets
   * can share a start, so the width is part of the key.
   */
  processesAt: (hostId: HostId, bucketT: number, bucketMs: number) =>
    [...historyKeys.host(hostId), "processes-at", bucketT, bucketMs] as const,
  /**
   * Per-app network bytes over `[fromMs, toMs)` (D-089). Under the history
   * family, so a Network history or retention change refetches it.
   */
  networkByApp: (hostId: HostId, fromMs: number, toMs: number) =>
    [...historyKeys.host(hostId), "network-by-app", fromMs, toMs] as const,
  /**
   * Per-app use over `[fromMs, toMs)`, the top `limit` by `by` (D-093,
   * D-099). Not stored history, but cleared with it, so it sits in the
   * family a clear invalidates.
   */
  usageByApp: (
    hostId: HostId,
    by: UsageKey,
    limit: number,
    fromMs: number,
    toMs: number
  ) =>
    [
      ...historyKeys.host(hostId),
      "usage-by-app",
      by,
      limit,
      fromMs,
      toMs,
    ] as const,
  /** Average, peak and integral of unlabelled series over `[fromMs, toMs)`. */
  seriesStats: (
    hostId: HostId,
    metrics: readonly string[],
    fromMs: number,
    toMs: number
  ) =>
    [
      ...historyKeys.host(hostId),
      "series-stats",
      metrics.join(","),
      fromMs,
      toMs,
    ] as const,
  /**
   * The 30-day heatmap: `hourStartMs` is the start of the current local
   * hour, so the key moves each hour (the newest cell closes) and at local
   * midnight (a new row), and fetches again then, never per tick.
   */
  heatmap: (
    hostId: HostId,
    metric: HeatmapMetric,
    hourStartMs: number,
    days: number
  ) =>
    [
      ...historyKeys.host(hostId),
      "heatmap",
      metric,
      hourStartMs,
      days,
    ] as const,
  /**
   * Detector and alert events over `spanMs` ending at `endMs` (`null`
   * follows now). `event-recorded` merges into every list under
   * `allEvents`, so a live view never refetches for a new one.
   */
  events: (hostId: HostId, spanMs: number, endMs: number | null) =>
    [...historyKeys.allEvents(hostId), spanMs, endMs] as const,
  allEvents: (hostId: HostId) =>
    [...historyKeys.host(hostId), "events"] as const,
  /** End of the latest sleep gap, for the machine header's "last wake". */
  lastWake: (hostId: HostId) =>
    [...historyKeys.host(hostId), "last-wake"] as const,
  /**
   * Gaps over the ring's hour, for the module pages' live charts. `paused`
   * is in the key so a pause or resume fetches again rather than leaving a
   * stale open "Paused" gap over new samples.
   */
  ringGaps: (hostId: HostId, paused: boolean) =>
    [...historyKeys.host(hostId), "ring-gaps", paused] as const,
};

export const hostKeys = {
  all: ["hosts"] as const,
  list: () => [...hostKeys.all, "list"] as const,
  detail: (hostId: HostId) => [...hostKeys.all, hostId, "record"] as const,
  capabilities: (hostId: HostId) =>
    [...hostKeys.all, hostId, "capabilities"] as const,
  historySize: (hostId: HostId) =>
    [...hostKeys.all, hostId, "history-size"] as const,
  /** Measured cost per day; under `historySize`, so the same changes refetch it. */
  historyGrowth: (hostId: HostId) =>
    [...hostKeys.historySize(hostId), "growth"] as const,
  /** Low-disk pause and size-limit trim; the event keeps it current. */
  historyHealth: (hostId: HostId) =>
    [...hostKeys.all, hostId, "history-health"] as const,
  sensorDump: (hostId: HostId) =>
    [...hostKeys.all, hostId, "sensor-dump"] as const,
  /** The primary interface's addresses; keyed by it, so a route change rereads. */
  networkAddresses: (hostId: HostId, iface: string | null) =>
    [...hostKeys.all, hostId, "addresses", iface] as const,
};

export const appKeys = {
  updates: ["app", "updates"] as const,
  appearance: ["app", "appearance"] as const,
  /** Build-time features (D-065); fixed for the life of the app. */
  edition: ["app", "edition"] as const,
  /**
   * The public address (D-093), keyed by the primary interface, its local
   * address and the egress interface: another network (a new lease, even on
   * the same Wi-Fi interface) or a VPN coming up asks again.
   */
  publicIp: (
    iface: string | null,
    local: string | null,
    egress: string | null
  ) => ["app", "public-ip", iface, local, egress] as const,
};

/** True for a query key that starts with `prefix`. */
function startsWith(key: readonly unknown[], prefix: readonly unknown[]) {
  return prefix.every((part, i) => key[i] === part);
}

const same = (a: unknown, b: unknown) =>
  JSON.stringify(a) === JSON.stringify(b);

/** Each module's on/off switch; the menu bar style changes no fetched data. */
const enabled = (s: Settings) =>
  Object.fromEntries(
    Object.entries(s.modules).map(([id, m]) => [id, m?.enabled])
  );

/**
 * Which cached queries a settings change makes stale, as a predicate over
 * query keys, per changed section:
 *
 * - `history` (retention, size limit): what history exists, its size on
 *   disk and the trim notice: every history key, history size and health.
 * - `modules`: a module switched off or on adds or closes a
 *   `module_disabled` gap and changes the series count: every history key
 *   and history size. Only `enabled` counts; the menu bar style does not.
 * - `sampling` (interval, slow on battery): rows per hour, so the size
 *   projection only; recorded history does not change.
 * - `general.check_updates`: the update check.
 *
 * Units, appearance, the other general switches and onboarding change no
 * fetched data. With no previous settings to compare against, every section
 * counts as changed.
 */
export function settingsInvalidation(
  prev: Settings | null,
  next: Settings
): (queryKey: readonly unknown[]) => boolean {
  const history = prev === null || !same(prev.history, next.history);
  const modules = prev === null || !same(enabled(prev), enabled(next));
  const sampling = prev === null || !same(prev.sampling, next.sampling);
  const updates =
    prev === null || prev.general.check_updates !== next.general.check_updates;
  return (key) => {
    if (startsWith(key, historyKeys.all)) return history || modules;
    if (startsWith(key, hostKeys.all)) {
      const part = key[2];
      if (part === "history-size") return history || modules || sampling;
      if (part === "history-health") return history;
      return false;
    }
    if (startsWith(key, appKeys.updates)) return updates;
    return false;
  };
}
