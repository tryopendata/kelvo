/**
 * View shaping for the per-app usage tables (D-099). Rust decides the
 * averages, peaks, grouping, the remainder and which process an app's Quit
 * acts on; this module filters, ranks for display and words shares.
 */
import type {
  AppUsage,
  ProcessUsage,
  UsageByApp,
  UsageKey,
} from "@core/generated/bindings";

/** The figures an app, a process or a remainder row carries. */
export type UsageFigures = Pick<
  AppUsage,
  | "cpu_avg_pct"
  | "gpu_avg_pct"
  | "mem_peak_bytes"
  | "read_bytes"
  | "write_bytes"
  | "energy_j"
  | "avg_w"
>;

/** The figure a table ranks by; null when it was not measured. */
export function usageValue(by: UsageKey, u: UsageFigures): number | null {
  switch (by) {
    case "cpu":
      return u.cpu_avg_pct;
    case "gpu":
      return u.gpu_avg_pct;
    case "memory":
      return u.mem_peak_bytes;
    case "disk":
      return u.read_bytes === null && u.write_bytes === null
        ? null
        : (u.read_bytes ?? 0) + (u.write_bytes ?? 0);
    case "energy":
      return u.energy_j;
  }
}

/** Keys whose rows add up, so shares and an "Other apps" row mean something. */
export const additive = (by: UsageKey) => by !== "memory";

export interface UsageRow {
  app: AppUsage;
  /** Percent of the whole (apps plus remainder); null with no share column. */
  share: number | null;
  /** The processes to list when expanded: all, or the ones the search matched. */
  processes: readonly ProcessUsage[];
  /** The search matched a process, not the app's name: show it expanded. */
  matchedInside: boolean;
  /** The process the app row's Quit acts on. */
  quit: ProcessUsage | null;
}

export interface RemainderRow {
  kind: "other" | "system";
  name: string;
  figures: UsageFigures;
  share: number | null;
}

const add = (a: number | null, b: number | null) =>
  a === null && b === null ? null : (a ?? 0) + (b ?? 0);

/**
 * What shares are of: every process's use plus what the host measured
 * beyond them, so the column adds up to 100%. Null for memory (peaks do not
 * add) or when nothing was used.
 */
export function usageWhole(data: UsageByApp, by: UsageKey): number | null {
  if (!additive(by)) return null;
  const total: UsageFigures = { ...data.total, mem_peak_bytes: 0 };
  const other: UsageFigures = {
    cpu_avg_pct: data.other.cpu_avg_pct,
    gpu_avg_pct: data.other.gpu_avg_pct,
    read_bytes: data.other.read_bytes,
    write_bytes: data.other.write_bytes,
    energy_j: null,
    avg_w: null,
    mem_peak_bytes: 0,
  };
  const whole = add(usageValue(by, total), usageValue(by, other));
  return whole !== null && whole > 0 ? whole : null;
}

export function shareOf(
  value: number | null,
  whole: number | null
): number | null {
  return value === null || whole === null ? null : (value / whole) * 100;
}

/**
 * Rows for the table, in Rust's order. A search is a case-insensitive
 * substring of the app or a process name, or the leading digits of a PID;
 * an app whose own name does not match keeps only the processes that do.
 */
export function usageRows(
  data: UsageByApp,
  by: UsageKey,
  query: string
): UsageRow[] {
  const whole = usageWhole(data, by);
  const q = query.trim().toLowerCase();
  const digits = /^\d+$/.test(q);
  const matches = (p: ProcessUsage) =>
    p.name.toLowerCase().includes(q) || (digits && String(p.pid).startsWith(q));
  const out: UsageRow[] = [];
  for (const app of data.apps) {
    const quit =
      app.quit_pid === null
        ? null
        : // A reused pid can list an exited process too; Rust picks a running one.
          (app.processes.find((p) => p.running && p.pid === app.quit_pid) ??
          null);
    const row = { app, share: shareOf(usageValue(by, app), whole), quit };
    if (!q || app.name.toLowerCase().includes(q)) {
      out.push({ ...row, processes: app.processes, matchedInside: false });
      continue;
    }
    const inside = app.processes.filter(matches);
    if (inside.length > 0) {
      out.push({ ...row, processes: inside, matchedInside: true });
    }
  }
  return out;
}

/**
 * The rows after the apps: "Other apps" (the processes below the table's
 * floor or past its limit) when they used a visible share, then "System
 * and other" (what the host measured beyond Kelvo's processes) when Rust
 * could read it. None for memory, whose peaks do not add.
 */
export function remainderRows(data: UsageByApp, by: UsageKey): RemainderRow[] {
  const whole = usageWhole(data, by);
  if (whole === null) return [];
  const listed = (f: (a: AppUsage) => number | null) =>
    data.apps.reduce((n, a) => n + (f(a) ?? 0), 0);
  const less = (t: number | null, f: (a: AppUsage) => number | null) =>
    t === null ? null : Math.max(0, t - listed(f));
  const t = data.total;
  const rest: UsageFigures = {
    cpu_avg_pct: less(t.cpu_avg_pct, (a) => a.cpu_avg_pct),
    gpu_avg_pct: less(t.gpu_avg_pct, (a) => a.gpu_avg_pct),
    read_bytes: less(t.read_bytes, (a) => a.read_bytes),
    write_bytes: less(t.write_bytes, (a) => a.write_bytes),
    energy_j: less(t.energy_j, (a) => a.energy_j),
    avg_w: null,
    mem_peak_bytes: 0,
  };
  const out: RemainderRow[] = [];
  const restValue = usageValue(by, rest);
  // Below 0.05% the row would read "0.0%".
  if (restValue !== null && restValue / whole >= 0.0005) {
    out.push({
      kind: "other",
      name: "Other apps",
      figures: rest,
      share: shareOf(restValue, whole),
    });
  }
  const system: UsageFigures = {
    cpu_avg_pct: data.other.cpu_avg_pct,
    gpu_avg_pct: data.other.gpu_avg_pct,
    read_bytes: data.other.read_bytes,
    write_bytes: data.other.write_bytes,
    energy_j: null,
    avg_w: null,
    mem_peak_bytes: 0,
  };
  const systemValue = usageValue(by, system);
  if (systemValue !== null) {
    out.push({
      kind: "system",
      name: "System and other",
      figures: system,
      share: shareOf(systemValue, whole),
    });
  }
  return out;
}

/** What the answer's coverage says about the range, for the notes over the table. */
export type UsageCoverage =
  /** No process sample yet. */
  | { kind: "waiting" }
  /** Nothing in the range was measured; `sinceMs` is when counting started. */
  | { kind: "unrecorded"; sinceMs: number | null }
  /** Counting started inside the range: the figures cover `coveredMs` from `sinceMs`. */
  | { kind: "partial"; sinceMs: number; coveredMs: number }
  /** Measured for less than the range (asleep, sampling paused). */
  | { kind: "gaps"; coveredMs: number; spanMs: number }
  | { kind: "full" };

/**
 * How much of the range the answer covers. Gaps are only judged once the
 * range is final, since its open tail is still being measured; 10 s or 1%
 * of slack absorbs the last sample's phase.
 */
export function usageCoverage(data: UsageByApp): UsageCoverage {
  if (data.since_ms === null) return { kind: "waiting" };
  if (data.covered_ms <= 0) {
    return {
      kind: "unrecorded",
      sinceMs: data.since_ms >= data.to_ms ? data.since_ms : null,
    };
  }
  if (data.since_ms > data.from_ms) {
    return {
      kind: "partial",
      sinceMs: data.since_ms,
      coveredMs: data.covered_ms,
    };
  }
  const span = data.to_ms - data.from_ms;
  const final =
    data.complete_to_ms !== null && data.complete_to_ms >= data.to_ms;
  if (final && data.covered_ms < span - Math.max(10_000, span * 0.01)) {
    return { kind: "gaps", coveredMs: data.covered_ms, spanMs: span };
  }
  return { kind: "full" };
}

/** An answer whose buckets no longer change: read once. */
export const usageFinal = (data: UsageByApp | undefined) =>
  data !== undefined &&
  data.complete_to_ms !== null &&
  data.complete_to_ms >= data.to_ms;

/** "4 processes" for an app row's collapsed summary. */
export function processCount(n: number): string {
  return `${n} ${n === 1 ? "process" : "processes"}`;
}
