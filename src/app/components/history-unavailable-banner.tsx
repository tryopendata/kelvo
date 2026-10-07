import { TriangleAlert } from "lucide-react";
import { Button } from "~/components/ui/button";

export interface HistoryUnavailableBannerProps {
  /** From `historyUnavailable` in `@core/history-state`. */
  message: string;
  /** Offer "Reset history"; passed when `historyUnavailable` says it can help. */
  onReset?: () => void;
}

/**
 * Banner over Timeline and history charts when the store failed to open or
 * write (plan 4.17). Live values keep working, and the banner says so.
 */
export function HistoryUnavailableBanner({
  message,
  onReset,
}: HistoryUnavailableBannerProps) {
  return (
    <div
      role="alert"
      className="flex items-center gap-2.5 rounded-tile border border-border bg-card px-3.5 py-2.5"
    >
      <TriangleAlert
        aria-hidden
        className="size-3.5 shrink-0 text-warning"
        strokeWidth={2}
      />
      <p className="m-0 min-w-0 flex-1 font-normal text-[12px] text-fg-subtle">
        {message}
      </p>
      {onReset && (
        <Button variant="outline" size="sm" onClick={onReset}>
          Reset history
        </Button>
      )}
    </div>
  );
}
