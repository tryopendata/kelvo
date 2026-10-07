import type { CommandError, HistoryHealth } from "@core/generated/bindings";
import {
  type HistoryNotice,
  historyHealthNotices,
  historyUnavailable,
} from "@core/history-state";
import { Info, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { HistoryUnavailableBanner } from "./history-unavailable-banner";
import { ResetHistoryDialog } from "./reset-history-dialog";

export interface HistoryNoticesProps {
  /** `undefined` for a chart that only needs the unavailable banner. */
  health: HistoryHealth | undefined;
  /**
   * A history command's error. When it says the store is unavailable the
   * banner replaces every other notice.
   */
  error?: CommandError | null;
  /** Range start of the chart below; see `historyHealthNotices`. */
  fromMs?: number;
}

/**
 * The store's state above a history view (plan 4.17, D-057, D-059): the
 * unavailable banner, the low-disk warning (icon, text and a border, so it
 * reads as a warning without relying on amber), and the trim note as one
 * muted line. Renders nothing when history is fine.
 */
export function HistoryNotices({ health, error, fromMs }: HistoryNoticesProps) {
  const [resetting, setResetting] = useState(false);
  const unavailable = error ? historyUnavailable(error) : null;
  if (unavailable) {
    return (
      <>
        <HistoryUnavailableBanner
          message={unavailable.message}
          onReset={unavailable.canReset ? () => setResetting(true) : undefined}
        />
        <ResetHistoryDialog open={resetting} onOpenChange={setResetting} />
      </>
    );
  }
  if (!health) return null;
  const notices = historyHealthNotices(health, fromMs);
  if (notices.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      {notices.map((n) => (
        <NoticeLine key={n.text} notice={n} />
      ))}
    </div>
  );
}

function NoticeLine({ notice }: { notice: HistoryNotice }) {
  if (notice.kind === "warning") {
    return (
      <div
        role="alert"
        className="flex items-start gap-2.5 rounded-tile border border-border bg-card px-3.5 py-2.5"
      >
        <TriangleAlert
          aria-hidden
          className="mt-px size-3.5 shrink-0 text-warning"
          strokeWidth={2}
        />
        <p className="m-0 min-w-0 flex-1 font-normal text-[12px] text-fg-subtle">
          {notice.text}
        </p>
      </div>
    );
  }
  return (
    <p
      role="status"
      className="m-0 flex items-start gap-2 font-normal text-[12px] text-muted-foreground"
    >
      <Info aria-hidden className="mt-px size-3.5 shrink-0" strokeWidth={1.5} />
      {notice.text}
    </p>
  );
}
