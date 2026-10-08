import { formatDuration } from "@core/format";
import { collectingHeader } from "@core/history-state";
import { cn } from "~/lib/utils";

/**
 * Header-right text of a collecting chart: "started 22:36 · 1
 * sample/s", from the first recorded sample and the sampling interval.
 */
export function CollectingNote({
  startedMs,
  intervalMs,
}: {
  startedMs: number;
  intervalMs: number;
}) {
  return (
    <span className="figures text-[11px] text-muted-foreground">
      {collectingHeader(startedMs, intervalMs)}
    </span>
  );
}

export interface CollectingOverlayProps {
  /** Span of history recorded so far, ms. */
  recordedMs: number;
  /** The `history.retention_days` setting. */
  retentionDays: number;
  className?: string;
}

/**
 * Empty-history notice centered over the lanes.
 * Says what is happening and how long it takes; no spinner, since nothing is
 * loading. Position it with `className` over the plot area.
 */
export function CollectingOverlay({
  recordedMs,
  retentionDays,
  className,
}: CollectingOverlayProps) {
  return (
    <div
      className={cn(
        "pointer-events-none flex items-center justify-center",
        className
      )}
    >
      <div
        role="status"
        className="pointer-events-auto flex flex-col items-center gap-1 rounded-tile border border-border bg-card px-4 py-3 text-center"
      >
        <span className="text-[13px]">
          Collecting · timeline fills in as you work
        </span>
        <span className="font-normal text-[12px] text-muted-foreground">
          First <span className="figures">{formatDuration(recordedMs)}</span>{" "}
          recorded. History is kept for{" "}
          <span className="figures">{retentionDays} days</span> on this Mac
          only.
        </span>
      </div>
    </div>
  );
}
