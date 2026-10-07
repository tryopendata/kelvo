/**
 * `query_energy_by_app` for the mock transport (D-093): the mock's own
 * processes grouped by app, each at its `energy` watts for the measured
 * span, plus two processes that exited during it. Mirrors
 * `src-tauri/src/energy.rs`: shares are of `total_j`, an app row's Quit goes
 * to its running main executable or its only running process, and exited
 * processes carry no refusal.
 */
import {
  type AppEnergy,
  USAGE_BUCKET_MS as BUCKET_MS,
  type EnergyByApp,
  type LiveProcess,
  type ProcessEnergy,
} from "@core/generated/bindings";
import { APP_OF } from "./usage";

/** Processes that ran during the window and have exited: `[app, name, pid, watts]`. */
const EXITED: readonly [string, string, number, number][] = [
  ["clang", "clang", 6120, 3.4],
  ["Xcode", "ibtoold", 6188, 0.6],
];

function floorTo(t: number): number {
  return t - (((t % BUCKET_MS) + BUCKET_MS) % BUCKET_MS);
}

export function mockEnergyByApp({
  processes,
  user,
  fromMs,
  toMs,
  sinceMs,
  latestMs,
}: {
  processes: readonly LiveProcess[];
  /** Only this user's processes are readable (D-045). */
  user: string;
  fromMs: number;
  toMs: number;
  /** When the mock started counting. */
  sinceMs: number;
  /** The newest process sample. */
  latestMs: number;
}): EnergyByApp {
  const from = floorTo(fromMs);
  const to = toMs % BUCKET_MS === 0 ? toMs : floorTo(toMs) + BUCKET_MS;
  const measuredMs = Math.max(
    0,
    Math.min(latestMs, to) - Math.max(sinceMs, from)
  );
  const secs = measuredMs / 1000;
  const avg = (j: number) => (secs > 0 ? j / secs : 0);

  const groups = new Map<string, ProcessEnergy[]>();
  const mains = new Map<string, number>();
  const add = (app: string, p: ProcessEnergy, main: boolean) => {
    const list = groups.get(app) ?? [];
    list.push(p);
    groups.set(app, list);
    if (main) mains.set(app, p.pid);
  };
  for (const p of processes) {
    if (p.user !== user || p.energy === null || p.energy <= 0) continue;
    const app = APP_OF[p.name] ?? p.name;
    const energy_j = p.energy * secs;
    add(
      app,
      {
        pid: p.pid,
        start_time_us: p.start_time_us,
        name: p.name,
        energy_j,
        avg_w: avg(energy_j),
        running: true,
        refusal: p.refusal,
      },
      app === p.name
    );
  }
  // The exited ones ran for the first third of the measured span.
  for (const [app, name, pid, watts] of EXITED) {
    const energy_j = (watts * secs) / 3;
    if (energy_j <= 0) continue;
    add(
      app,
      {
        pid,
        start_time_us: 1_759_500_000_000_000 + pid * 1_000_000,
        name,
        energy_j,
        avg_w: avg(energy_j),
        running: false,
        refusal: null,
      },
      false
    );
  }

  const apps: AppEnergy[] = [...groups].map(([name, list]) => {
    const j = (p: ProcessEnergy) => p.energy_j ?? 0;
    list.sort((a, b) => j(b) - j(a) || a.pid - b.pid);
    const energy_j = list.reduce((s, p) => s + j(p), 0);
    const running = list.filter((p) => p.running);
    const main = mains.get(name);
    return {
      name,
      energy_j,
      avg_w: avg(energy_j),
      quit_pid:
        main !== undefined
          ? main
          : running.length === 1
            ? (running[0]?.pid ?? null)
            : null,
      processes: list,
    };
  });
  const j = (a: AppEnergy) => a.energy_j ?? 0;
  apps.sort((a, b) => j(b) - j(a) || a.name.localeCompare(b.name));
  return {
    from_ms: from,
    to_ms: to,
    since_ms: sinceMs,
    measured_ms: measuredMs,
    total_j: apps.reduce((s, a) => s + j(a), 0),
    apps,
  };
}
