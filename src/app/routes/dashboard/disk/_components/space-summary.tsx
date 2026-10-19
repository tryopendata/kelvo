import { bytesParts, formatBytes, formatSpan } from "@core/format";
import type { SpaceReport, SpaceScanStatus } from "@core/generated/bindings";
import { ChevronRight } from "lucide-react";
import { Button } from "~/components/ui/button";
import { useNow } from "~/hooks/use-now";
import { MeterTrack } from "~/widgets/meter-track";
import { StatStrip } from "~/widgets/stat-strip";
import {
  agoLabel,
  elapsedLabel,
  itemsLabel,
  middleTruncate,
} from "../_lib/space";

/**
 * A running scan (plan "Core action and feedback", item 4): bytes measured
 * against used space, labelled "about" because unattributed space can stop
 * it short of 100%, then items, time and the folder being read.
 */
export function SpaceProgress({
  status,
  onCancel,
}: {
  status: SpaceScanStatus;
  onCancel: () => void;
}) {
  const used = status.used_bytes ?? 0;
  const fraction = used > 0 ? (status.measured_bytes ?? 0) / used : null;
  const pct = Math.min(99, Math.floor((fraction ?? 0) * 100));
  return (
    <div role="status" className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2">
        <p className="figures text-[13px] text-foreground">
          Scanning · about {pct}% measured
          <span className="text-fg-subtle">
            {" · "}
            {itemsLabel(status.items)} · {elapsedLabel(status.elapsed_ms)}
          </span>
        </p>
        <Button size="sm" variant="outline" onClick={onCancel}>
          Cancel
        </Button>
      </div>
      <MeterTrack fraction={fraction} fill="var(--color-disk)" height="h-1.5" />
      <p className="flex min-w-0 flex-wrap justify-between gap-x-6 gap-y-1 text-[11px] text-muted-foreground">
        <span className="min-w-0 truncate" title={status.current ?? ""}>
          Reading {middleTruncate(`/${status.current ?? ""}`, 72)}
        </span>
        <span>
          Kelvo shows as a top disk reader in Activity while it scans.
        </span>
      </p>
    </div>
  );
}

/** The finished report's figures (plan "The finished report", item 1). */
export function SpaceSummary({
  report,
  onRescan,
  busy,
}: {
  report: SpaceReport;
  onRescan: () => void;
  busy: boolean;
}) {
  const now = useNow(30_000);
  const used = bytesParts(report.used_bytes ?? 0);
  const items = [
    { label: "In files", value: formatBytes(report.files_bytes) },
    ...((report.volumes_bytes ?? 0) > 0
      ? [{ label: "Other volumes", value: formatBytes(report.volumes_bytes) }]
      : []),
    {
      label: "Not attributed",
      value:
        report.other_bytes === null ? "—" : formatBytes(report.other_bytes),
      muted: true,
    },
    {
      label: "Scanned",
      value: agoLabel(now, report.finished_ms),
      secondary: `${formatSpan(report.duration_ms)} · ${itemsLabel(report.items)}`,
    },
  ];
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-end justify-between gap-x-6 gap-y-3">
        <StatStrip
          hero={{ label: "Used", value: used.value, unit: used.unit }}
          items={items}
        />
        <Button size="sm" variant="outline" disabled={busy} onClick={onRescan}>
          Scan again
        </Button>
      </div>
      <SpaceNotes report={report} />
    </div>
  );
}

function SpaceNotes({ report }: { report: SpaceReport }) {
  const notes: string[] = [];
  if (report.skipped.length > 0)
    notes.push(
      `Skipped, protected by macOS: ${report.skipped.join(", ")}. Their size is part of Not attributed.`
    );
  if (report.truncated)
    notes.push(
      "Very deep folders are summarized; every total is still complete."
    );
  if (notes.length === 0 && report.unreadable === 0) return null;
  return (
    <div className="flex flex-col gap-1 text-[11px] text-muted-foreground">
      {notes.map((n) => (
        <p key={n}>{n}</p>
      ))}
      {report.unreadable > 0 && (
        <details className="group">
          <summary className="flex w-fit cursor-default list-none items-center gap-1 rounded-sm outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden">
            <ChevronRight
              aria-hidden
              className="size-3 transition-transform duration-(--motion-fast) group-open:rotate-90"
            />
            {report.unreadable === 1
              ? "1 folder could not be read"
              : `${report.unreadable} folders could not be read`}
          </summary>
          <ul className="mt-1 flex flex-col gap-0.5 pl-4">
            {report.unreadable_paths.map((p) => (
              <li key={p} className="truncate">
                /{p}
              </li>
            ))}
            {report.unreadable > report.unreadable_paths.length && (
              <li>
                and {report.unreadable - report.unreadable_paths.length} more
              </li>
            )}
          </ul>
        </details>
      )}
    </div>
  );
}
