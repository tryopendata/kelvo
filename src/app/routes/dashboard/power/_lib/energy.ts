import type {
  AppEnergy,
  EnergyByApp,
  ProcessEnergy,
} from "@core/generated/bindings";

/**
 * View shaping for the energy table (D-093). Rust decides the totals,
 * averages, grouping and which process an app's Quit acts on; this module
 * only filters and words them.
 */

export interface EnergyRow {
  app: AppEnergy;
  /** Share of every app's energy, percent; null when nothing was used. */
  share: number | null;
  /** The processes to list when expanded: all, or the ones the search matched. */
  processes: readonly ProcessEnergy[];
  /** The search matched a process, not the app's name: show it expanded. */
  matchedInside: boolean;
  /** The process the app row's Quit acts on. */
  quit: ProcessEnergy | null;
}

const share = (j: number | null, total: number | null) =>
  j === null || total === null || total <= 0 ? null : (j / total) * 100;

/**
 * Rows for the table, largest first as Rust sent them. A search is a
 * case-insensitive substring of the app or a process name, or the leading
 * digits of a PID; an app whose own name does not match keeps only the
 * processes that do.
 */
export function energyRows(data: EnergyByApp, query: string): EnergyRow[] {
  const q = query.trim().toLowerCase();
  const digits = /^\d+$/.test(q);
  const matches = (p: ProcessEnergy) =>
    p.name.toLowerCase().includes(q) || (digits && String(p.pid).startsWith(q));
  const out: EnergyRow[] = [];
  for (const app of data.apps) {
    const quit =
      app.quit_pid === null
        ? null
        : // A reused pid can list an exited process too; Rust picks a running one.
          (app.processes.find((p) => p.running && p.pid === app.quit_pid) ??
          null);
    const row = { app, share: share(app.energy_j, data.total_j), quit };
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

export function processShare(p: ProcessEnergy, data: EnergyByApp) {
  return share(p.energy_j, data.total_j);
}

/**
 * How much of the window was measured, when not all of it: Kelvo keeps
 * this in memory only, so it starts at launch (or after History is
 * cleared). Null when the whole window is covered.
 */
export function partialSince(
  data: EnergyByApp,
  windowMs: number
): { sinceMs: number; measuredMs: number } | null {
  if (data.since_ms === null || data.since_ms <= data.from_ms) return null;
  if (data.measured_ms >= windowMs) return null;
  return { sinceMs: data.since_ms, measuredMs: data.measured_ms };
}

/** "4 processes" for an app row's collapsed summary. */
export function processCount(n: number): string {
  return `${n} ${n === 1 ? "process" : "processes"}`;
}
