import type { TimeRange } from "@core/brush";
import { formatClockSeconds } from "@core/format";
import { XIcon } from "lucide-react";
import { useBrushStore } from "~/stores/brush-store";

/**
 * "14:02:10 to 14:03:40 ×": the pinned selection in a table's header, with
 * a dismiss icon in place of "Clear" text (D-093).
 */
export function SelectionChip({ range }: { range: TimeRange }) {
  const store = useBrushStore();
  return (
    <span className="inline-flex h-6 items-center gap-1.5 rounded-full border border-border bg-btn pl-2.5 text-[11px] text-foreground">
      <span className="figures">{formatClockSeconds(range.fromMs)}</span>
      <span className="text-muted-foreground">to</span>
      <span className="figures">{formatClockSeconds(range.toMs)}</span>
      <button
        type="button"
        onClick={() => store.getState().clear()}
        aria-label="Clear selection"
        title="Clear selection (Esc)"
        className="grid h-5.5 place-items-center rounded-r-full border-border-subtle border-l pr-2 pl-1.5 text-fg-subtle outline-none hover:bg-selected hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        <XIcon aria-hidden className="size-3" />
      </button>
    </span>
  );
}
