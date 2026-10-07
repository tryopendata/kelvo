/**
 * History states shared by every history chart (plan 4.17): gap
 * bands and their labels by reason (the Timeline and module charts both use
 * these), the "collecting" threshold for a range that is
 * mostly empty, the banner text when the store is unavailable, and the
 * low-disk and size-limit notices (D-057, D-059).
 */

import { formatDuration } from "@core/format";
import type {
  CommandError,
  Gap,
  HistoryHealth,
  Module,
} from "@core/generated/bindings";
import { sizeLimitLabel } from "@core/history-projection";

/**
 * How often the store's writer commits (`kelvo_store::DEFAULT_COMMIT_INTERVAL`,
 * D-070), generated from Rust. Stored processes and heatmap hours newer than
 * this may not be readable yet; `query_history` and `battery_hours` answer
 * through now (D-092).
 */
export { HISTORY_COMMIT_MS } from "@core/generated/bindings";

/** Wall-clock "HH:MM" in the viewer's zone. */
export function clockTime(ms: number): string {
  const d = new Date(ms);
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

const MODULE_LABEL: Partial<Record<Module, string>> = {
  cpu: "CPU",
  gpu: "GPU",
  memory: "Memory",
  power: "Power",
  sensors: "Sensor",
  network: "Network",
  disk: "Disk",
  battery: "Battery",
};

/**
 * How a closed sleep gap names its span: `clock` gives both times, where
 * they fit (1h: "Asleep 11:02–11:31 · not interpolated");
 * `duration` gives the length, for wide ranges (24h: "Asleep
 * 5h 15m · no samples").
 */
export type GapLabelStyle = "clock" | "duration";

/**
 * The band label for one gap (plan 4.17): "Asleep 11:02–11:31 · not
 * interpolated", "Kelvo not running", "Paused", "CPU sampling off". An open
 * sleep gap (asleep now) has no end, so it says when it started. A reason
 * this build does not know (`unknown`, or one a newer engine added) is still
 * a gap and gets the generic label.
 */
export function gapLabel(gap: Gap, style: GapLabelStyle = "clock"): string {
  // A string switch: the engine may send reasons newer than these bindings.
  const reason: string = gap.reason;
  switch (reason) {
    case "sleep":
      if (gap.end_ms === null) return `Asleep since ${clockTime(gap.start_ms)}`;
      return style === "clock"
        ? `Asleep ${clockTime(gap.start_ms)}–${clockTime(gap.end_ms)} · not interpolated`
        : `Asleep ${formatDuration(gap.end_ms - gap.start_ms)} · no samples`;
    case "app_not_running":
      return "Kelvo not running";
    case "paused":
      return "Paused";
    case "module_disabled": {
      const name = gap.module ? MODULE_LABEL[gap.module] : undefined;
      return `${name ?? "Module"} sampling off`;
    }
    case "clock_changed":
      return "Clock changed";
    case "write_failed":
      return "History write failed";
    case "truncated":
      return "History pruned";
    case "source_offline":
      return "Host offline";
    default:
      return "No samples";
  }
}

/**
 * One gap per span. Each chart or lane queries its own history page, so the
 * same whole-host gap can come back once per page; two gaps with the same
 * reason and module that overlap are one gap, kept with the later end (an
 * open gap wins).
 */
export function dedupeGaps(gaps: readonly Gap[]): Gap[] {
  const out: Gap[] = [];
  const sorted = [...gaps].sort((a, b) => a.start_ms - b.start_ms);
  for (const g of sorted) {
    const same = out.find(
      (k) =>
        k.reason === g.reason &&
        k.module === g.module &&
        g.start_ms <= (k.end_ms ?? Infinity) &&
        (g.end_ms ?? Infinity) >= k.start_ms
    );
    if (!same) {
      out.push({ ...g });
    } else if (same.end_ms !== null) {
      same.end_ms = g.end_ms === null ? null : Math.max(same.end_ms, g.end_ms);
    }
  }
  return out;
}

export interface GapBandSpec {
  fromMs: number;
  toMs: number;
  /** Visible label and accessible name. */
  label: string;
  /** Set for a module-scoped gap (`module_disabled`): it covers that module only. */
  module: Module | null;
}

export interface GapBandOptions {
  /**
   * The module the chart shows: keep whole-host gaps and this module's
   * gaps. Omitted (the Timeline, one band row over every lane) keeps all.
   */
  module?: Module;
  style?: GapLabelStyle;
}

/**
 * Bands over `[fromMs, toMs)`, deduplicated, clipped to the range and in
 * time order; an open gap (asleep now, paused now) runs to the range end.
 */
export function gapBands(
  gaps: readonly Gap[],
  fromMs: number,
  toMs: number,
  { module, style = "clock" }: GapBandOptions = {}
): GapBandSpec[] {
  const out: GapBandSpec[] = [];
  for (const g of dedupeGaps(gaps)) {
    if (module !== undefined && g.module !== null && g.module !== module) {
      continue;
    }
    const start = Math.max(fromMs, g.start_ms);
    const end = Math.min(toMs, g.end_ms ?? toMs);
    if (end <= start) continue;
    out.push({
      fromMs: start,
      toMs: end,
      label: gapLabel(g, style),
      module: g.module,
    });
  }
  return out.sort((a, b) => a.fromMs - b.fromMs);
}

/**
 * Proposed threshold (plan 4.17): a range is "collecting" while the recorded
 * span inside it is under a quarter of the range.
 */
export const COLLECTING_FRACTION = 0.25;

/**
 * True when history in `[fromMs, toMs]` starts so late that the chart would
 * be mostly empty. `recordedFromMs` is the first recorded sample, `null`
 * when there is none yet.
 */
export function isCollecting(
  recordedFromMs: number | null,
  fromMs: number,
  toMs: number
): boolean {
  const range = toMs - fromMs;
  if (range <= 0) return false;
  if (recordedFromMs === null) return true;
  const recorded = toMs - Math.max(fromMs, recordedFromMs);
  return recorded < range * COLLECTING_FRACTION;
}

/** "started 22:36 · 1 sample/s" for a collecting chart's header. */
export function collectingHeader(
  recordedFromMs: number,
  intervalMs: number
): string {
  const perSecond = 1000 / intervalMs;
  const rate =
    perSecond >= 1
      ? `${Number.isInteger(perSecond) ? perSecond : perSecond.toFixed(1)} sample/s`
      : `1 sample/${Math.round(intervalMs / 1000)}s`;
  return `started ${clockTime(recordedFromMs)} · ${rate}`;
}

/** What the unavailable banner says, and whether "Reset history" can help. */
export interface HistoryUnavailable {
  message: string;
  /**
   * `reset_history` moves the file aside and starts an empty one. It cannot
   * help while another Kelvo holds the file (`locked`), nor with a single
   * failed query (`store`), so the banner offers it only for the rest.
   */
  canReset: boolean;
}

const LIVE = "Live values still work.";

const tooNew = (found: number, supported: number): HistoryUnavailable => ({
  message: `History was written by a newer version of Kelvo (format ${found}; this version reads up to ${supported}). ${LIVE}`,
  canReset: true,
});

const corrupt = (): HistoryUnavailable => ({
  message: `The history file is damaged. ${LIVE}`,
  canReset: true,
});

/**
 * The banner for a history command's error, or null for errors that are
 * not about the store (unknown host, a bad argument, a busy writer worth
 * a retry). Reasons are read as strings: a newer engine may add one, and
 * it is still "unavailable" with a reset on offer.
 */
export function historyUnavailable(
  error: CommandError
): HistoryUnavailable | null {
  switch (error.kind) {
    case "history_unavailable": {
      const reason = error.reason ?? null;
      const kind: string | null = reason?.kind ?? null;
      if (reason?.kind === "too_new") {
        return tooNew(reason.found, reason.supported);
      }
      if (kind === "corrupt") return corrupt();
      if (kind === "locked") {
        return {
          message: `History is unavailable: another copy of Kelvo has the history file open. Quit it, then reopen Kelvo. ${LIVE}`,
          canReset: false,
        };
      }
      if (reason?.kind === "failed") {
        return {
          message: `History is unavailable: ${reason.message}. ${LIVE}`,
          canReset: true,
        };
      }
      return { message: `History is unavailable. ${LIVE}`, canReset: true };
    }
    case "store_corrupt":
      return corrupt();
    case "store_too_new":
      return tooNew(error.found, error.supported);
    case "store":
      return {
        message: `History is unavailable: ${error.message}. ${LIVE}`,
        canReset: false,
      };
    default:
      return null;
  }
}

/**
 * What the Apps card says when `query_network_by_app` fails (D-089): the
 * store's own banner text where there is one, otherwise a line per kind.
 */
export function networkByAppFailure(error: CommandError): string {
  const store = historyUnavailable(error);
  if (store) return store.message;
  switch (error.kind) {
    case "remote_host":
      return "Per-app network history is kept only for this Mac.";
    case "unknown_host":
      return "This host is no longer connected.";
    case "store_busy":
      return "History is busy. App totals will load on the next refresh.";
    default:
      return "message" in error
        ? `Couldn't load app totals: ${error.message}.`
        : "Couldn't load app totals.";
  }
}

/** The toast after `reset_history` fails, by error kind. */
export function resetHistoryFailure(error: CommandError): string {
  const locked =
    error.kind === "store_busy" ||
    (error.kind === "history_unavailable" && error.reason?.kind === "locked");
  if (locked) {
    return "Another copy of Kelvo has the history file open. Quit it and try again.";
  }
  return "Couldn't reset history.";
}

const MONTHS = "Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec".split(" ");

/** "Sep 12 14:02" in the viewer's zone. */
function dayTime(ms: number): string {
  const d = new Date(ms);
  return `${MONTHS[d.getMonth()]} ${d.getDate()} ${clockTime(ms)}`;
}

/**
 * A notice about the store itself. A warning means history is not being
 * kept as asked (disk almost full, or over the limit even at one day); info
 * explains why history starts later than the retention says.
 */
export interface HistoryNotice {
  kind: "warning" | "info";
  text: string;
}

/**
 * The notices for `health`. With `fromMs` (a chart's range start) the trim
 * note is shown only when the range reaches back before where history now
 * starts, so a 24 h chart does not mention a trim it cannot see. Without it
 * (Settings) every notice is shown.
 */
export function historyHealthNotices(
  health: HistoryHealth,
  fromMs?: number
): HistoryNotice[] {
  const out: HistoryNotice[] = [];
  if (health.low_disk_paused) {
    out.push({
      kind: "warning",
      text: "History paused: disk almost full. 10-second detail resumes when space frees up; minute history and live values continue.",
    });
  }
  const trimmed = health.trimmed_before_ms;
  const limit =
    health.trimmed_limit_bytes === null
      ? "the size limit"
      : sizeLimitLabel(Math.round(health.trimmed_limit_bytes / 1e6));
  if (!health.cap_met) {
    out.push({
      kind: "warning",
      text: `History is over ${limit} even with only the last day kept.`,
    });
  } else if (trimmed !== null && (fromMs === undefined || trimmed > fromMs)) {
    out.push({
      kind: "info",
      text: `History trimmed to stay under ${limit}. It starts ${dayTime(trimmed)}.`,
    });
  }
  return out;
}
