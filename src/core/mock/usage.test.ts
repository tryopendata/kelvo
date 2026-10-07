import type { LiveProcess, SeriesStats } from "@core/generated/bindings";
import { describe, expect, it } from "vitest";
import { mockUsageByApp } from "./usage";

const MIB = 1024 * 1024;

function proc(
  pid: number,
  name: string,
  over: Partial<LiveProcess> = {}
): LiveProcess {
  return {
    pid,
    start_time_us: pid * 1_000,
    name,
    cpu_pct: 0,
    mem_bytes: 0,
    compressed_bytes: null,
    threads: 1,
    idle_wakeups_per_s: 0,
    energy: 0,
    disk_read_bps: 0,
    disk_write_bps: 0,
    net_rx_bps: null,
    net_tx_bps: null,
    gpu_pct: null,
    ports: null,
    user: "me",
    refusal: null,
    ...over,
  };
}

function stats(metrics: [string, number][]): SeriesStats {
  return {
    from_ms: 0,
    to_ms: 60_000,
    metrics: metrics.map(([metric, avg]) => ({
      metric,
      measured_ms: 60_000,
      avg,
      max: avg,
      integral: avg * 60,
    })),
  };
}

const base = {
  user: "me",
  fromMs: 0,
  toMs: 60_000,
  sinceMs: 0,
  latestMs: 60_000,
  gpu: false,
  stats: null,
  cores: 8,
} as const;

describe("mockUsageByApp", () => {
  it("groups helpers under their app and keeps the limit by the key", () => {
    const r = mockUsageByApp({
      ...base,
      processes: [
        proc(1, "Google Chrome", { cpu_pct: 10 }),
        proc(2, "Google Chrome Helper (Renderer)", { cpu_pct: 30 }),
        proc(3, "node", { cpu_pct: 25 }),
        proc(4, "launchd", { cpu_pct: 90, user: "root" }),
      ],
      by: "cpu",
      limit: 2,
    });
    expect(r.apps.map((a) => a.name)).toEqual(["clang", "Google Chrome"]);
    expect(r.apps[1]?.cpu_avg_pct).toBe(40);
    expect(r.apps[1]?.quit_pid).toBe(1);
    // Root's processes are not readable: they are in nobody's row.
    expect(r.total.cpu_avg_pct).toBeCloseTo(10 + 30 + 25 + 60 + 35 / 3);
    expect(r.covered_ms).toBe(60_000);
  });

  it("sums an app's memory and keeps children above their own floor", () => {
    const helpers = Array.from({ length: 30 }, (_, i) =>
      proc(100 + i, "Slack Helper", { mem_bytes: 20 * MIB })
    );
    const r = mockUsageByApp({
      ...base,
      processes: helpers,
      by: "memory",
      limit: 10,
    });
    const slack = r.apps.find((a) => a.name === "Slack");
    expect(slack?.mem_peak_bytes).toBe(600 * MIB);
    expect(slack?.processes).toEqual([]);
  });

  it("takes the remainder from the host series and clamps it", () => {
    const r = mockUsageByApp({
      ...base,
      processes: [proc(1, "node", { cpu_pct: 50, disk_write_bps: 1_000 })],
      by: "disk",
      limit: 10,
      // 25% of 8 cores is 200% of one core; the disk wrote less than the apps.
      stats: stats([
        ["cpu.total", 25],
        ["disk.write_total", 100],
      ]),
    });
    expect(r.other.cpu_avg_pct).toBeCloseTo(200 - 50 - 60 - 35 / 3);
    expect(r.other.write_bytes).toBe(0);
    expect(r.other.clamped).toEqual(["disk"]);
    expect(r.other.gpu_avg_pct).toBeNull();
  });

  it("reads as unmeasured before counting started", () => {
    const r = mockUsageByApp({
      ...base,
      sinceMs: 120_000,
      latestMs: 180_000,
      processes: [proc(1, "node", { cpu_pct: 50 })],
      by: "cpu",
      limit: 10,
    });
    expect(r.covered_ms).toBe(0);
  });
});
