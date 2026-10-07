import {
  formatPercent,
  formatTemperature,
  monthName,
  pad2,
  WEEKDAYS_2,
} from "@core/format";
import type {
  HeatmapDay,
  HeatmapDaySpec,
  HeatmapMetric,
} from "@core/generated/bindings";
import type { LaneUnits } from "./format";

/**
 * How a heatmap metric maps onto cell opacity: `lo` is the faintest cell,
 * `hi` and above the strongest.
 *
 * CPU is 0 to 80%: an hour averaging above 80% is rare and
 * already alarming, so the ramp spends its range below that.
 *
 * Temperature is the hottest SoC zone, 40 to 90 °C: an idle Apple Silicon
 * Mac sits around 40 °C, so idle hours are the faint end, and 90 °C is
 * where sustained load starts to throttle, so a saturated cell means an hour
 * spent near the limit. Hours outside the range clamp to its ends.
 */
export interface HeatmapScale {
  lo: number;
  hi: number;
  /** The Timeline lane's accent: CPU cyan, temperature amber. */
  accent: "cpu" | "temp";
}

export const HEATMAP_SCALES: Record<HeatmapMetric, HeatmapScale> = {
  cpu: { lo: 0, hi: 80, accent: "cpu" },
  temp: { lo: 40, hi: 90, accent: "temp" },
};

/** The five legend swatches, faint to strong. */
export const LEGEND_ALPHAS = [0.08, 0.28, 0.5, 0.72, 0.95] as const;

/**
 * A cell's fill opacity: `0.06 + (v / 80) * 0.89` for CPU, capped at
 * 0.95, generalised to the scale's `lo` and `hi`.
 */
export function cellAlpha(value: number, scale: HeatmapScale): number {
  const f = (value - scale.lo) / (scale.hi - scale.lo);
  return Math.min(0.95, 0.06 + Math.max(0, f) * 0.89);
}

/** The legend's ends: "0%" and "80%+", or "40°C" and "90°C+". */
export function legendEnds(
  metric: HeatmapMetric,
  units: LaneUnits
): [string, string] {
  const { lo, hi } = HEATMAP_SCALES[metric];
  const f = (v: number) =>
    metric === "cpu"
      ? formatPercent(v)
      : formatTemperature(v, { units: units.temperature });
  return [f(lo), `${f(hi)}+`];
}

/** `2026-09-05` as a local date. */
function parseIso(date: string): Date {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y as number, (m as number) - 1, d as number);
}

/** The row label, "Sep 05 Sa". */
export function heatmapDayLabel(date: string): string {
  const d = parseIso(date);
  return `${monthName(d)} ${pad2(d.getDate())} ${WEEKDAYS_2[d.getDay()]}`;
}

/** One heatmap row: the day's boundaries from the request, its values from the reply. */
export interface HeatmapRow {
  date: string;
  label: string;
  /** 25 local hour boundaries, ms. */
  hourStarts: number[];
  /** 24 values; `null` is an hour without samples. */
  hours: (number | null)[];
}

/** Joins the request's days to the reply by date; a day missing from the reply is all empty. */
export function heatmapRows(
  specs: readonly HeatmapDaySpec[],
  days: readonly HeatmapDay[] | undefined
): HeatmapRow[] {
  const byDate = new Map((days ?? []).map((d) => [d.date, d.hours]));
  return specs.map((s) => ({
    date: s.date,
    label: heatmapDayLabel(s.date),
    hourStarts: s.hour_starts,
    hours: Array.from(
      { length: 24 },
      (_, h) => byDate.get(s.date)?.[h] ?? null
    ),
  }));
}

/** The cell holding `nowMs`, as `[row, hour]`, or `null` when it is outside the rows. */
export function currentCell(
  rows: readonly HeatmapRow[],
  nowMs: number
): [number, number] | null {
  for (let r = rows.length - 1; r >= 0; r--) {
    const s = (rows[r] as HeatmapRow).hourStarts;
    for (let h = 0; h < 24; h++) {
      if (nowMs >= (s[h] as number) && nowMs < (s[h + 1] as number)) {
        return [r, h];
      }
    }
  }
  return null;
}

/**
 * A cell that starts after `nowHourMs` (the start of the current local
 * hour): it has not happened yet, so it is neither data nor a gap.
 */
export function isFutureCell(
  row: HeatmapRow,
  hour: number,
  nowHourMs: number
): boolean {
  return (row.hourStarts[hour] as number) > nowHourMs;
}

/**
 * The current hour's cell while it has no value: its minutes are not
 * committed yet (D-070), so right after the hour turns, or after launch, it
 * is not a gap. Drawn and named like a future cell, but it still opens.
 */
export function isPendingCell(
  row: HeatmapRow,
  hour: number,
  nowHourMs: number
): boolean {
  return (
    row.hours[hour] == null &&
    (row.hourStarts[hour] as number) <= nowHourMs &&
    nowHourMs < (row.hourStarts[hour + 1] as number)
  );
}

/** What a cell shows besides its value: still loading, or not yet happened. */
export interface CellContext {
  nowHourMs: number;
  loading: boolean;
}

/**
 * "Sep 22, 14:00, average CPU 31%", "Sep 22, 14:00, no samples", "Oct 5,
 * 23:00, not yet" or "Sep 22, 14:00, loading".
 */
export function cellName(
  row: HeatmapRow,
  hour: number,
  metric: HeatmapMetric,
  units: LaneUnits,
  ctx: CellContext
): string {
  const d = parseIso(row.date);
  const when = `${monthName(d)} ${d.getDate()}, ${pad2(hour)}:00`;
  if (isFutureCell(row, hour, ctx.nowHourMs)) return `${when}, not yet`;
  if (ctx.loading) return `${when}, loading`;
  if (isPendingCell(row, hour, ctx.nowHourMs)) {
    return `${when}, this hour, no data yet`;
  }
  const v = row.hours[hour];
  if (v == null) return `${when}, no samples`;
  return metric === "cpu"
    ? `${when}, average CPU ${formatPercent(v)}`
    : `${when}, average temperature ${formatTemperature(v, { units: units.temperature })}`;
}
