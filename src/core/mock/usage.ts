/**
 * `query_usage_by_app` for the mock transport (D-099): the mock's own
 * processes grouped by app, each holding its current rates over the covered
 * span, plus two that exited during it. Mirrors `usage.rs` in the engine and
 * the app shell: children are kept above a floor and apps above a looser one,
 * totals count every process, an app row's Quit goes to its running main
 * executable or its only running process, exited processes carry no refusal,
 * and `other` is the host's series less the apps' totals, clamped at 0.
 */
import type {
  AppUsage,
  LiveProcess,
  ProcessUsage,
  SeriesStats,
  UsageByApp,
  UsageKey,
} from "@core/generated/bindings";
import { USAGE_BUCKET_MS as BUCKET_MS } from "@core/generated/bindings";

/** Helpers and services, by the app the identity rule charges them to. */
export const APP_OF: Record<string, string> = {
  "Safari Web Content": "Safari",
  SourceKitService: "Xcode",
  "Code Helper (Renderer)": "Code",
  "Google Chrome Helper (Renderer)": "Google Chrome",
  "Slack Helper": "Slack",
  "com.apple.WebKit.WebContent": "Safari",
};

/**
 * Processes that ran during the window and have exited, for the first third
 * of the covered span: `[app, name, pid, watts, cpu %, MiB, write B/s]`.
 */
export const EXITED: readonly [
  string,
  string,
  number,
  number,
  number,
  number,
  number,
][] = [
  ["clang", "clang", 6120, 3.4, 180, 420, 12e6],
  ["Xcode", "ibtoold", 6188, 0.6, 35, 180, 0],
];

const MIB = 1024 * 1024;

/** The engine's floors (`usage.rs`), on rates over the span. */
function clears(
  cpuPct: number,
  diskBps: number,
  gpuPct: number,
  watts: number,
  memBytes: number,
  memFloor: number
): boolean {
  return (
    cpuPct >= 0.1 ||
    diskBps >= 1024 ||
    gpuPct > 0 ||
    watts >= 1e-5 ||
    memBytes >= memFloor
  );
}

function floorTo(t: number): number {
  return t - (((t % BUCKET_MS) + BUCKET_MS) % BUCKET_MS);
}

/** The value apps and processes sort by. */
function keyOf(
  by: UsageKey,
  u: Pick<
    ProcessUsage,
    | "cpu_avg_pct"
    | "gpu_avg_pct"
    | "mem_peak_bytes"
    | "read_bytes"
    | "write_bytes"
    | "energy_j"
  >
): number {
  switch (by) {
    case "cpu":
      return u.cpu_avg_pct ?? 0;
    case "gpu":
      return u.gpu_avg_pct ?? 0;
    case "memory":
      return u.mem_peak_bytes;
    case "disk":
      return (u.read_bytes ?? 0) + (u.write_bytes ?? 0);
    case "energy":
      return u.energy_j ?? 0;
  }
}

/** `ProcessUsage` with the amounts the mock computes known to be numbers. */
type Proc = Omit<
  ProcessUsage,
  "cpu_avg_pct" | "read_bytes" | "write_bytes" | "energy_j" | "avg_w"
> & {
  cpu_avg_pct: number;
  read_bytes: number;
  write_bytes: number;
  energy_j: number;
  avg_w: number;
};

/** One mock process over the span: what it used, and for how much of it. */
interface Row {
  app: string;
  main: boolean;
  share: number;
  proc: Proc;
  watts: number;
  memBytes: number;
}

export function mockUsageByApp({
  processes,
  user,
  fromMs,
  toMs,
  sinceMs,
  latestMs,
  by,
  limit,
  gpu,
  stats,
  cores,
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
  by: UsageKey;
  limit: number;
  /** Per-process GPU is sampled (`process_gpu`). */
  gpu: boolean;
  /** The host series over the covered part, `null` when unreadable. */
  stats: SeriesStats | null;
  cores: number;
}): UsageByApp {
  const from = floorTo(fromMs);
  const to = toMs % BUCKET_MS === 0 ? toMs : floorTo(toMs) + BUCKET_MS;
  const coveredMs = Math.max(
    0,
    Math.min(latestMs, to) - Math.max(sinceMs, from)
  );
  const secs = coveredMs / 1000;
  const gpuCoveredMs = gpu ? coveredMs : 0;

  const rows: Row[] = [];
  const add = (
    app: string,
    main: boolean,
    share: number,
    p: {
      pid: number;
      start_time_us: number;
      name: string;
      cpu: number;
      gpu: number | null;
      memBytes: number;
      readBps: number;
      writeBps: number;
      watts: number;
      running: boolean;
      refusal: LiveProcess["refusal"];
    }
  ) => {
    const s = secs * share;
    const energy_j = p.watts * s;
    rows.push({
      app,
      main,
      share,
      watts: p.watts * share,
      memBytes: p.memBytes,
      proc: {
        pid: p.pid,
        start_time_us: p.start_time_us,
        name: p.name,
        cpu_avg_pct: p.cpu * share,
        gpu_avg_pct: gpu ? (p.gpu ?? 0) * share : null,
        mem_peak_bytes: p.memBytes,
        read_bytes: p.readBps * s,
        write_bytes: p.writeBps * s,
        energy_j,
        avg_w: secs > 0 ? energy_j / secs : 0,
        running: p.running,
        refusal: p.running ? p.refusal : null,
      },
    });
  };
  for (const p of processes) {
    if (p.user !== user) continue;
    const app = APP_OF[p.name] ?? p.name;
    add(app, app === p.name, 1, {
      pid: p.pid,
      start_time_us: p.start_time_us,
      name: p.name,
      cpu: p.cpu_pct ?? 0,
      gpu: p.gpu_pct,
      memBytes: p.mem_bytes,
      readBps: p.disk_read_bps ?? 0,
      writeBps: p.disk_write_bps ?? 0,
      watts: p.energy ?? 0,
      running: true,
      refusal: p.refusal,
    });
  }
  for (const [app, name, pid, watts, cpu, mib, writeBps] of EXITED) {
    add(app, false, 1 / 3, {
      pid,
      start_time_us: 1_759_500_000_000_000 + pid * 1_000_000,
      name,
      cpu,
      gpu: null,
      memBytes: mib * MIB,
      readBps: 0,
      writeBps,
      watts,
      running: false,
      refusal: null,
    });
  }

  const total = {
    cpu_avg_pct: 0,
    gpu_avg_pct: gpu ? 0 : null,
    read_bytes: 0,
    write_bytes: 0,
    energy_j: 0,
    avg_w: 0,
  };
  const groups = new Map<string, Row[]>();
  for (const r of rows) {
    total.cpu_avg_pct += r.proc.cpu_avg_pct;
    if (total.gpu_avg_pct !== null) {
      total.gpu_avg_pct += r.proc.gpu_avg_pct ?? 0;
    }
    total.read_bytes += r.proc.read_bytes;
    total.write_bytes += r.proc.write_bytes;
    total.energy_j += r.proc.energy_j;
    const list = groups.get(r.app) ?? [];
    list.push(r);
    groups.set(r.app, list);
  }
  total.avg_w = secs > 0 ? total.energy_j / secs : 0;

  const apps: AppUsage[] = [];
  for (const [name, list] of groups) {
    const sum = (f: (r: Row) => number) => list.reduce((s, r) => s + f(r), 0);
    const cpu = sum((r) => r.proc.cpu_avg_pct);
    const gpuPct = gpu ? sum((r) => r.proc.gpu_avg_pct ?? 0) : null;
    const read = sum((r) => r.proc.read_bytes);
    const write = sum((r) => r.proc.write_bytes);
    const watts = sum((r) => r.watts);
    // Running processes coexist; the exited ones ran together, earlier.
    const running = list.filter((r) => r.proc.running);
    const exited = list.filter((r) => !r.proc.running);
    const memNow = running.reduce((s, r) => s + r.memBytes, 0);
    const memThen = exited.reduce((s, r) => s + r.memBytes, 0) + memNow;
    const peak = exited.length > 0 ? memThen : memNow;
    if (
      !clears(
        cpu,
        secs > 0 ? (read + write) / secs : 0,
        gpuPct ?? 0,
        watts,
        peak,
        8 * MIB
      )
    ) {
      continue;
    }
    const usage = {
      cpu_avg_pct: cpu,
      gpu_avg_pct: gpuPct,
      mem_peak_bytes: peak,
      read_bytes: read,
      write_bytes: write,
      energy_j: sum((r) => r.proc.energy_j),
    };
    const processesKept = list
      .filter((r) =>
        clears(
          r.proc.cpu_avg_pct,
          secs > 0 ? (r.proc.read_bytes + r.proc.write_bytes) / secs : 0,
          r.proc.gpu_avg_pct ?? 0,
          r.watts,
          r.memBytes,
          32 * MIB
        )
      )
      .map((r) => r.proc)
      .sort((a, b) => keyOf(by, b) - keyOf(by, a) || a.pid - b.pid);
    const main = running.find((r) => r.main);
    apps.push({
      name,
      ...usage,
      mem_avg_bytes:
        exited.length > 0
          ? Math.round(memNow + (memThen - memNow) / 3)
          : memNow,
      avg_w: secs > 0 ? usage.energy_j / secs : 0,
      quit_pid:
        main !== undefined
          ? main.proc.pid
          : running.length === 1
            ? (running[0]?.proc.pid ?? null)
            : null,
      processes: processesKept,
    });
  }
  apps.sort(
    (a, b) => keyOf(by, b) - keyOf(by, a) || a.name.localeCompare(b.name)
  );

  const stat = (m: string) =>
    stats?.metrics.find((s) => s.metric === m && s.measured_ms > 0) ?? null;
  let clamped = false;
  const less = (host: number, apps: number, slack: number) => {
    const d = host - apps;
    if (d < -slack) clamped = true;
    return Math.max(0, d);
  };
  const measured = coveredMs > 0;
  const cpuHost = stat("cpu.total")?.avg ?? null;
  const gpuHost = stat("gpu.util")?.avg ?? null;
  const read = stat("disk.read_total");
  const write = stat("disk.write_total");
  const other = {
    cpu_avg_pct:
      measured && cores > 0 && cpuHost !== null
        ? less(cpuHost * cores, total.cpu_avg_pct, 1)
        : null,
    gpu_avg_pct:
      gpuHost !== null && total.gpu_avg_pct !== null
        ? less(gpuHost, total.gpu_avg_pct, 1)
        : null,
    read_bytes:
      measured && read
        ? less(
            read.integral ?? 0,
            total.read_bytes,
            0.01 * (read.integral ?? 0)
          )
        : null,
    write_bytes:
      measured && write
        ? less(
            write.integral ?? 0,
            total.write_bytes,
            0.01 * (write.integral ?? 0)
          )
        : null,
    clamped: false,
  };
  other.clamped = clamped;

  return {
    from_ms: from,
    to_ms: to,
    since_ms: sinceMs,
    covered_ms: coveredMs,
    gpu_covered_ms: gpuCoveredMs,
    total,
    other,
    apps: apps.slice(0, limit),
  };
}
