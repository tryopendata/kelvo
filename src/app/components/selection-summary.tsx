import type { TimeRange } from "@core/brush";
import { formatClockSeconds } from "@core/format";
import { PLOT_INSET } from "~/components/charts/layout";
import { useBrushRange } from "~/stores/brush-store";
import { useHost } from "~/stores/host-store";

/**
 * The line under a brushable chart: the hint with no selection, otherwise
 * the selected range (its figures are in the views it scopes), and
 * "earlier than this chart" once the range has scrolled off. `shown` is the
 * range an answer covers when that is wider than the selection; `inset`
 * lines the text up with the plot.
 */
export function SelectionSummary({
  windowMs,
  shown,
  inset = PLOT_INSET,
}: {
  windowMs: number;
  shown?: TimeRange | null;
  inset?: number;
}) {
  const range = useBrushRange();
  // A boolean, so the line re-renders when the range leaves the chart, not per tick.
  const offChart = useHost(
    (s) =>
      range !== null &&
      s.lastTsMs !== null &&
      range.toMs <= s.lastTsMs - windowMs
  );
  const className = "mt-1 mb-0 font-normal text-[12px] text-muted-foreground";

  if (range === null) {
    return (
      <p className={className} style={{ paddingLeft: inset }}>
        Drag across the chart to see what used it.
      </p>
    );
  }
  const r = shown ?? range;
  return (
    <p
      data-testid="selection-summary"
      className={className}
      style={{ paddingLeft: inset }}
    >
      <span className="figures text-foreground">
        {formatClockSeconds(r.fromMs)}
      </span>{" "}
      to{" "}
      <span className="figures text-foreground">
        {formatClockSeconds(r.toMs)}
      </span>
      {offChart && " · earlier than this chart"}
      <span className="text-fg-faint">
        {" "}
        · Click the chart or press Esc to clear
      </span>
    </p>
  );
}
