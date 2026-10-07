import type { CommandError, ExportOutcome } from "@core/generated/bindings";
import { localDateIso } from "@core/heatmap-days";
import { historyUnavailable } from "@core/history-state";
import type { Span } from "./time";

const pad = (n: number) => String(n).padStart(2, "0");

/**
 * The name the save dialog proposes: the range's local start and its
 * length, "kelvo-2026-10-04-2240-24h.csv". Rust only drops path separators,
 * so the local date is ours to format.
 */
export function exportFileName(span: Span, fromMs: number): string {
  const d = new Date(fromMs);
  return `kelvo-${localDateIso(d)}-${pad(d.getHours())}${pad(d.getMinutes())}-${span}.csv`;
}

/** The toast after a save: where it went and how much. `null` for a cancel. */
export function exportSaved(outcome: ExportOutcome): string | null {
  if (outcome.kind === "cancelled") return null;
  const rows = `${outcome.rows.toLocaleString()} ${outcome.rows === 1 ? "row" : "rows"}`;
  const gaps =
    outcome.gap_rows > 0
      ? ` and ${outcome.gap_rows} ${outcome.gap_rows === 1 ? "gap" : "gaps"}`
      : "";
  return `Exported ${rows}${gaps} to ${outcome.path}`;
}

/** The toast when the command could not run at all (a rejected invoke). */
export function exportThrown(thrown: unknown): string {
  const message = thrown instanceof Error ? thrown.message : String(thrown);
  return `Export failed: ${message}`;
}

/** The toast when the export failed. */
export function exportFailure(error: CommandError): string {
  if (historyUnavailable(error)) {
    return "Export failed: no history is being kept.";
  }
  switch (error.kind) {
    case "store_busy":
      return "Export failed: the history file is busy. Try again in a moment.";
    case "export":
      return `Export failed: ${error.message}`;
    default:
      return "message" in error
        ? `Export failed (${error.kind}): ${error.message}`
        : `Export failed (${error.kind}).`;
  }
}
