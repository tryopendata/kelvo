import type { HeatmapDaySpec } from "@core/generated/bindings";
import { createMockTransport } from "../mock-transport";
import { MOCK_HOST_ID } from "./fixtures";
import { mockCpuHours, mockHeatmap } from "./heatmap";

const HOUR = 3_600_000;
// 2026-10-05T00:00:00Z, a Monday.
const MONDAY = Date.UTC(2026, 9, 5);

/** A 24-hour day of one-hour cells from `start`. */
function day(date: string, start: number): HeatmapDaySpec {
  return {
    date,
    hour_starts: Array.from({ length: 25 }, (_, h) => start + h * HOUR),
  };
}

describe("mock heatmap", () => {
  it("is the same for a date whatever range asks for it", () => {
    expect(mockCpuHours("2026-10-05")).toEqual(mockCpuHours("2026-10-05"));
    expect(mockCpuHours("2026-10-05")).not.toEqual(mockCpuHours("2026-10-06"));
  });

  it("is asleep at night, busier on weekday working hours", () => {
    const weekdays = ["2026-09-28", "2026-09-29", "2026-09-30", "2026-10-01"];
    for (const d of weekdays) {
      const hours = mockCpuHours(d);
      expect(hours).toHaveLength(24);
      // 02:00 to 06:59 is always asleep: hatched, never 0.
      expect(hours.slice(2, 7)).toEqual([null, null, null, null, null]);
      expect(hours.slice(7).every((v) => v !== null && v > 0)).toBe(true);
    }
    const mean = (vs: (number | null)[]) =>
      vs.reduce<number>((a, v) => a + (v ?? 0), 0) / vs.length;
    const work = weekdays.map((d) => mean(mockCpuHours(d).slice(9, 19)));
    const evening = weekdays.map((d) => mean(mockCpuHours(d).slice(19)));
    expect(mean(work)).toBeGreaterThan(mean(evening));
  });

  it("leaves future hours and empty cells null, and maps temperature", () => {
    const now = MONDAY + 12 * HOUR + 30 * 60_000;
    const spec = day("2026-10-05", MONDAY);
    // A spring-forward style empty cell at hour 9.
    spec.hour_starts[9] = spec.hour_starts[10] as number;
    const [cpu] = mockHeatmap(
      { host: MOCK_HOST_ID, metric: "cpu", days: [spec] },
      now
    );
    expect(cpu?.hours.slice(13)).toEqual(Array(11).fill(null));
    expect(cpu?.hours[12]).not.toBeNull();
    expect(cpu?.hours[9]).toBeNull();

    const [temp] = mockHeatmap(
      { host: MOCK_HOST_ID, metric: "temp", days: [spec] },
      now
    );
    const v = cpu?.hours[10] as number;
    expect(temp?.hours[10]).toBeCloseTo(38 + v * 0.6, 0);
  });
});

describe("mock transport heatmap and export", () => {
  it("answers query_heatmap per requested day and records it", async () => {
    const t = createMockTransport({ now: () => MONDAY + 23 * HOUR });
    const request = {
      host: MOCK_HOST_ID,
      metric: "cpu" as const,
      days: [day("2026-10-04", MONDAY - 24 * HOUR), day("2026-10-05", MONDAY)],
    };
    const res = await t.queryHeatmap(request);
    expect(res.status).toBe("ok");
    if (res.status !== "ok") return;
    expect(res.data.map((d) => d.date)).toEqual(["2026-10-04", "2026-10-05"]);
    expect(res.data[0]?.hours).toEqual(mockCpuHours("2026-10-04"));
    expect(t.calls.at(-1)).toEqual({
      command: "query_heatmap",
      args: [request],
    });

    const bad = await t.queryHeatmap({
      ...request,
      days: [{ date: "x", hour_starts: [MONDAY] }],
    });
    expect(bad.status === "error" && bad.error.kind).toBe("invalid_argument");
  });

  it("fails like Rust when history is unavailable", async () => {
    const t = createMockTransport({ scenarios: ["history-unavailable"] });
    const res = await t.queryHeatmap({
      host: MOCK_HOST_ID,
      metric: "temp",
      days: [],
    });
    expect(res.status === "error" && res.error.kind).toBe(
      "history_unavailable"
    );
  });

  it("export_csv saves to a fake path, or is cancelled, and records the request", async () => {
    const request = {
      host: MOCK_HOST_ID,
      selectors: [{ metric: "cpu.total", labels: [] }],
      from_ms: MONDAY - 7 * 24 * HOUR,
      to_ms: MONDAY,
      tier: "auto" as const,
      file_name: "kelvo-2026-10-05.csv",
    };
    const t = createMockTransport({ now: () => MONDAY });
    const res = await t.exportCsv(request);
    expect(res).toMatchObject({
      status: "ok",
      data: {
        kind: "saved",
        path: "/Users/mock/Downloads/kelvo-2026-10-05.csv",
        rows: 7 * 24 * 60,
      },
    });
    expect(t.calls.at(-1)).toEqual({ command: "export_csv", args: [request] });

    const cancelling = createMockTransport({ exportCancels: true });
    expect(await cancelling.exportCsv(request)).toEqual({
      status: "ok",
      data: { kind: "cancelled" },
    });

    const empty = await t.exportCsv({ ...request, selectors: [] });
    expect(empty.status === "error" && empty.error.kind).toBe(
      "invalid_argument"
    );
  });
});
