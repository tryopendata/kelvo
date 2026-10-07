import { formatClockSeconds } from "@core/format";
import { useBrushRange } from "~/stores/brush-store";
import { useHost } from "~/stores/host-store";
import { useNetworkByApp } from "../_hooks/use-network-by-app";

/**
 * The line under the brushable chart: the hint with no
 * selection, otherwise the selected range (its bytes are in the totals
 * above the chart), and
 * "earlier than this chart" once the range has scrolled off.
 */
export function SelectionSummary({ windowMs }: { windowMs: number }) {
  const range = useBrushRange();
  const { data } = useNetworkByApp(range);
  // A boolean, so the line re-renders when the range leaves the chart, not per tick.
  const offChart = useHost(
    (s) =>
      range !== null &&
      s.lastTsMs !== null &&
      range.toMs <= s.lastTsMs - windowMs
  );

  if (range === null) {
    return (
      <p className="mt-1 mb-0 pl-24 font-normal text-[12px] text-muted-foreground">
        Drag across the chart to see what used it.
      </p>
    );
  }
  const from = data?.from_ms ?? range.fromMs;
  const to = data?.to_ms ?? range.toMs;
  return (
    <p
      data-testid="selection-summary"
      className="mt-1 mb-0 pl-24 font-normal text-[12px] text-muted-foreground"
    >
      <span className="data-mono text-foreground">
        {formatClockSeconds(from)}
      </span>{" "}
      to{" "}
      <span className="data-mono text-foreground">
        {formatClockSeconds(to)}
      </span>
      {offChart && " · earlier than this chart"}
      <span className="text-fg-faint">
        {" "}
        · Click the chart or press Esc to clear
      </span>
    </p>
  );
}
