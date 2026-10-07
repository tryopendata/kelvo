import type { AppUsage, UsageByApp } from "@core/generated/bindings";
import {
  remainderRows,
  usageCoverage,
  usageFinal,
  usageRows,
} from "./usage-rows";

const T = 1_700_000_000_000;

function app(
  name: string,
  cpu: number,
  over: Partial<AppUsage> = {}
): AppUsage {
  return {
    name,
    cpu_avg_pct: cpu,
    gpu_avg_pct: null,
    mem_peak_bytes: 0,
    mem_avg_bytes: 0,
    read_bytes: 0,
    write_bytes: 0,
    energy_j: 0,
    avg_w: 0,
    quit_pid: null,
    processes: [],
    ...over,
  };
}

function answer(over: Partial<UsageByApp> = {}): UsageByApp {
  return {
    from_ms: T,
    to_ms: T + 60_000,
    since_ms: T - 600_000,
    complete_to_ms: T + 60_000,
    covered_ms: 60_000,
    gpu_covered_ms: 0,
    total: {
      cpu_avg_pct: 100,
      gpu_avg_pct: null,
      read_bytes: 0,
      write_bytes: 0,
      energy_j: 0,
      avg_w: 0,
    },
    other: {
      cpu_avg_pct: 100,
      gpu_avg_pct: null,
      read_bytes: null,
      write_bytes: null,
      clamped: [],
    },
    apps: [app("Xcode", 60), app("node", 30)],
    ...over,
  };
}

describe("usage rows (D-099)", () => {
  it("shares are of every process plus the host's remainder", () => {
    const rows = usageRows(answer(), "cpu", "");
    expect(rows.map((r) => [r.app.name, r.share])).toEqual([
      ["Xcode", 30],
      ["node", 15],
    ]);
    // 10 points of the 100 went to apps below the floor.
    expect(
      remainderRows(answer(), "cpu").map((r) => [r.name, r.share])
    ).toEqual([
      ["Other apps", 5],
      ["System and other", 50],
    ]);
  });

  it("memory has no shares and no remainder: peaks do not add", () => {
    expect(usageRows(answer(), "memory", "")[0]?.share).toBeNull();
    expect(remainderRows(answer(), "memory")).toEqual([]);
  });

  it("a search keeps the processes that match inside an app", () => {
    const proc = {
      pid: 4120,
      start_time_us: 1,
      name: "SourceKitService",
      cpu_avg_pct: 20,
      gpu_avg_pct: null,
      mem_peak_bytes: 0,
      read_bytes: 0,
      write_bytes: 0,
      energy_j: 0,
      avg_w: 0,
      running: true,
      refusal: null,
    };
    const data = answer({
      apps: [app("Xcode", 60, { processes: [proc] }), app("node", 30)],
    });
    const rows = usageRows(data, "cpu", "41");
    expect(rows.map((r) => [r.app.name, r.matchedInside])).toEqual([
      ["Xcode", true],
    ]);
  });

  it("names what the answer covers", () => {
    expect(usageCoverage(answer({ since_ms: null })).kind).toBe("waiting");
    expect(
      usageCoverage(answer({ covered_ms: 0, since_ms: T + 120_000 }))
    ).toEqual({ kind: "unrecorded", sinceMs: T + 120_000 });
    expect(
      usageCoverage(answer({ since_ms: T + 20_000, covered_ms: 40_000 }))
    ).toEqual({ kind: "partial", sinceMs: T + 20_000, coveredMs: 40_000 });
    expect(usageCoverage(answer({ covered_ms: 30_000 })).kind).toBe("gaps");
    // Still being measured: a short covered time is the open tail, not a gap.
    expect(
      usageCoverage(answer({ covered_ms: 30_000, complete_to_ms: T })).kind
    ).toBe("full");
  });

  it("an answer is final once its buckets are complete", () => {
    expect(usageFinal(answer())).toBe(true);
    expect(usageFinal(answer({ complete_to_ms: T + 50_000 }))).toBe(false);
    expect(usageFinal(undefined)).toBe(false);
  });
});
