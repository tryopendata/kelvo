import { heatmapDays } from "@core/heatmap-days";
import {
  cellAlpha,
  cellName,
  currentCell,
  HEATMAP_SCALES,
  heatmapDayLabel,
  heatmapRows,
  isFutureCell,
  isPendingCell,
  legendEnds,
} from "./heatmap";

const C = { rate: "MBps", temperature: "C" } as const;
const F = { rate: "MBps", temperature: "F" } as const;

describe("cellAlpha", () => {
  it("follows the CPU ramp: 0.06 at 0%, capped at 0.95 from 80%", () => {
    const cpu = HEATMAP_SCALES.cpu;
    expect(cellAlpha(0, cpu)).toBeCloseTo(0.06);
    expect(cellAlpha(40, cpu)).toBeCloseTo(0.06 + 0.5 * 0.89);
    expect(cellAlpha(80, cpu)).toBeCloseTo(0.95);
    expect(cellAlpha(100, cpu)).toBeCloseTo(0.95);
  });

  it("maps temperature from 40 to 90 °C and clamps below", () => {
    const temp = HEATMAP_SCALES.temp;
    expect(cellAlpha(30, temp)).toBeCloseTo(0.06);
    expect(cellAlpha(40, temp)).toBeCloseTo(0.06);
    expect(cellAlpha(65, temp)).toBeCloseTo(0.06 + 0.5 * 0.89);
    expect(cellAlpha(95, temp)).toBeCloseTo(0.95);
  });
});

describe("labels", () => {
  it('names rows like "Sep 05 Sa"', () => {
    expect(heatmapDayLabel("2026-09-05")).toBe("Sep 05 Sa");
    expect(heatmapDayLabel("2026-10-04")).toBe("Oct 04 Su");
  });

  it("gives the legend ends in the display unit", () => {
    expect(legendEnds("cpu", C)).toEqual(["0%", "80%+"]);
    expect(legendEnds("temp", C)).toEqual(["40 °C", "90 °C+"]);
    expect(legendEnds("temp", F)).toEqual(["104 °F", "194 °F+"]);
  });

  it("names a cell by date, hour and value, or no samples", () => {
    const [spec] = heatmapDays(new Date(2026, 8, 22, 12), 1);
    if (!spec) throw new Error("no day");
    const hours = Array.from({ length: 24 }, (_, h) =>
      h === 14 ? 31.4 : null
    );
    const [row] = heatmapRows([spec], [{ date: spec.date, hours }]);
    if (!row) throw new Error("no row");
    // Seen the next day: every hour has happened.
    const later = {
      nowHourMs: new Date(2026, 8, 23, 9).getTime(),
      loading: false,
    };
    expect(cellName(row, 14, "cpu", C, later)).toBe(
      "Sep 22, 14:00, average CPU 31%"
    );
    expect(cellName(row, 3, "cpu", C, later)).toBe("Sep 22, 03:00, no samples");
    expect(cellName(row, 14, "temp", C, later)).toBe(
      "Sep 22, 14:00, average temperature 31 °C"
    );
    expect(cellName(row, 3, "cpu", C, { ...later, loading: true })).toBe(
      "Sep 22, 03:00, loading"
    );
  });

  it("names the current hour without a value 'no data yet', not 'no samples'", () => {
    const [spec] = heatmapDays(new Date(2026, 9, 5, 14, 2), 1);
    if (!spec) throw new Error("no day");
    // 14:02: hour 14's minutes are not committed yet, so no row for it.
    const hours = Array.from({ length: 24 }, (_, h) => (h < 14 ? 20 : null));
    const [row] = heatmapRows([spec], [{ date: spec.date, hours }]);
    if (!row) throw new Error("no row");
    const ctx = {
      nowHourMs: new Date(2026, 9, 5, 14).getTime(),
      loading: false,
    };
    expect(isPendingCell(row, 14, ctx.nowHourMs)).toBe(true);
    expect(isPendingCell(row, 13, ctx.nowHourMs)).toBe(false);
    expect(isPendingCell(row, 15, ctx.nowHourMs)).toBe(false);
    expect(cellName(row, 14, "cpu", C, ctx)).toBe(
      "Oct 5, 14:00, this hour, no data yet"
    );
    expect(isFutureCell(row, 14, ctx.nowHourMs)).toBe(false);
    // Once its minutes land, it is an ordinary value.
    const [filled] = heatmapRows(
      [spec],
      [{ date: spec.date, hours: hours.map((v, h) => (h === 14 ? 30 : v)) }]
    );
    if (!filled) throw new Error("no row");
    expect(isPendingCell(filled, 14, ctx.nowHourMs)).toBe(false);
    expect(cellName(filled, 14, "cpu", C, ctx)).toBe(
      "Oct 5, 14:00, average CPU 30%"
    );
  });

  it("names the hours after the current one 'not yet', not 'no samples'", () => {
    const [spec] = heatmapDays(new Date(2026, 9, 5, 12), 1);
    if (!spec) throw new Error("no day");
    const [row] = heatmapRows([spec], undefined);
    if (!row) throw new Error("no row");
    const ctx = {
      nowHourMs: new Date(2026, 9, 5, 22).getTime(),
      loading: false,
    };
    expect(isFutureCell(row, 22, ctx.nowHourMs)).toBe(false);
    expect(isFutureCell(row, 23, ctx.nowHourMs)).toBe(true);
    expect(cellName(row, 21, "cpu", C, ctx)).toBe("Oct 5, 21:00, no samples");
    expect(cellName(row, 22, "cpu", C, ctx)).toBe(
      "Oct 5, 22:00, this hour, no data yet"
    );
    expect(cellName(row, 23, "cpu", C, ctx)).toBe("Oct 5, 23:00, not yet");
    // Loading does not hide that an hour is still ahead.
    expect(cellName(row, 23, "cpu", C, { ...ctx, loading: true })).toBe(
      "Oct 5, 23:00, not yet"
    );
  });
});

describe("heatmapRows and currentCell", () => {
  const now = new Date(2026, 9, 4, 14, 20);
  const specs = heatmapDays(now, 30);

  it("joins the reply by date and leaves a missing day empty, never zero", () => {
    const rows = heatmapRows(specs, [
      { date: specs[29]?.date ?? "", hours: Array(24).fill(10) },
    ]);
    expect(rows).toHaveLength(30);
    expect(rows[29]?.hours.every((v) => v === 10)).toBe(true);
    expect(rows[0]?.hours.every((v) => v === null)).toBe(true);
    expect(rows[0]?.label).toBe("Sep 05 Sa");
  });

  it("finds the cell holding now in the last row", () => {
    const rows = heatmapRows(specs, undefined);
    expect(currentCell(rows, now.getTime())).toEqual([29, 14]);
    expect(currentCell(rows, now.getTime() + 2 * 86_400_000)).toBeNull();
  });
});
