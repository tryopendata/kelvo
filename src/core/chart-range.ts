import type { ChartWindow } from "@core/generated/bindings";
import { RETENTION_DAYS } from "@core/generated/bindings";
import {
  CHART_WINDOW_MS,
  windowAllowed,
  windowLabel,
  windowWords,
} from "@core/live-window";

export const MAX_RANGE_MS = Math.max(...RETENTION_DAYS) * 86_400_000;
export const RANGE_PRESETS = [
  5, 15, 30, 60, 180, 360, 720, 1440, 2880, 10080, 20160, 43200, 129600,
].map((minutes) => ({
  ms: minutes * 60_000,
  label: `Last ${minutes === 60 ? "1 hour" : minutes === 1440 ? "1 day" : windowWords(minutes * 60_000)}`,
}));

export function chartRange(value: ChartWindow): {
  windowMs: number;
  endMs: number | null;
} {
  if (typeof value === "string")
    return { windowMs: CHART_WINDOW_MS[value], endMs: null };
  if ("relative" in value) return { windowMs: value.relative, endMs: null };
  return {
    windowMs: value.absolute.to_ms - value.absolute.from_ms,
    endMs: value.absolute.to_ms,
  };
}

export function relativeWindow(ms: number): ChartWindow {
  const preset = Object.entries(CHART_WINDOW_MS).find(
    ([, duration]) => duration === ms
  );
  return preset
    ? (preset[0] as Extract<ChartWindow, string>)
    : { relative: ms };
}

export function validChartRange(value: ChartWindow): boolean {
  const { windowMs, endMs } = chartRange(value);
  return (
    Number.isSafeInteger(windowMs) &&
    windowMs >= 60_000 &&
    windowMs <= MAX_RANGE_MS &&
    (endMs === null ||
      (Number.isSafeInteger(endMs) &&
        endMs <= Number.MAX_SAFE_INTEGER &&
        endMs - windowMs >= 0))
  );
}

export function chartRangeLabel(value: ChartWindow): string {
  const range = chartRange(value);
  return range.endMs === null ? windowLabel(range.windowMs) : "Custom";
}

/** Local, unambiguous editable wall time; the visible timezone sits beside the fields. */
export function dateInput(ms: number): string {
  const date = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** Recorded bounds retain seconds and fractional precision; editable fields stay minutes. */
export function recordedDate(ms: number): string {
  return recordedWallTime(ms, Math.round((ms % 1000) * 1000));
}
export function recordedProcessStart(us: number): string {
  return recordedWallTime(Math.floor(us / 1000), us % 1_000_000);
}
function recordedWallTime(ms: number, fractionUs: number): string {
  const date = new Date(ms);
  const seconds = date.getSeconds();
  if (seconds === 0 && fractionUs === 0) return dateInput(ms);
  const fraction =
    fractionUs === 0
      ? ""
      : `.${String(fractionUs).padStart(6, "0").replace(/0+$/, "")}`;
  return `${dateInput(ms)}:${String(seconds).padStart(2, "0")}${fraction}`;
}

export function parseDateInput(text: string): number | null {
  if (!/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/.test(text)) return null;
  const ms = new Date(text.replace(" ", "T")).getTime();
  return Number.isFinite(ms) && dateInput(ms) === text ? ms : null;
}

/** Validate a draft before it can change the applied range. */
export function customRange(draft: {
  mode: string;
  amount: string;
  unitMs: number;
  from: string;
  to: string;
  now: number;
  intervalMs: number;
}): { value: ChartWindow } | { error: string } {
  if (draft.mode === "relative") {
    const ms = Number(draft.amount) * draft.unitMs;
    if (!Number.isSafeInteger(ms) || ms < 60_000 || ms > MAX_RANGE_MS)
      return { error: "Choose a duration from 1 minute to 90 days." };
    if (!windowAllowed(ms, draft.intervalMs))
      return {
        error: "Choose a longer range for the current sampling interval.",
      };
    return { value: relativeWindow(ms) };
  }
  const fromMs = parseDateInput(draft.from);
  const toMs = parseDateInput(draft.to);
  if (fromMs === null || toMs === null)
    return { error: "Enter valid dates and times as YYYY-MM-DD HH:mm." };
  if (toMs > draft.now)
    return { error: "The end time must be now or earlier." };
  const value: ChartWindow = { absolute: { from_ms: fromMs, to_ms: toMs } };
  if (!validChartRange(value))
    return {
      error:
        "Choose a range from 1 minute to 90 days, with the start before the end.",
    };
  return { value };
}
