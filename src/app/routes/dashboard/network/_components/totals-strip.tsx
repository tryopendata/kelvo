import { windowRange } from "@core/brush";
import { formatBytes, formatSpan, MISSING } from "@core/format";
import { useOpenEdge } from "~/hooks/use-range-scope";
import { useBrushRange } from "~/stores/brush-store";
import { StatStrip } from "~/widgets/stat-strip";
import { useNetworkTotals } from "../_hooks/use-network-totals";
import { totalsMeasured } from "../_lib/network";

/**
 * Bytes down and up over the chart window (D-091), or over the brushed range
 * when there is one, beside the live rates on the throughput card. The window
 * ends where the open 10 s bucket starts, so the figures change every 10 s.
 * "in 11 min" follows a figure when part of the range wasn't measured.
 */
export function TotalsStrip({ windowMs }: { windowMs: number }) {
  const selection = useBrushRange();
  const edge = useOpenEdge();
  const range =
    selection ?? (edge === null ? null : windowRange(edge, windowMs));
  const totals = useNetworkTotals(range, {
    keepPrevious: selection === null,
  });
  // A held answer stands in only for the window it was read for (the key
  // moving every 10 s), never for a selection that was just cleared.
  const shown =
    totals.isPlaceholderData &&
    totals.data &&
    totals.data.to_ms - totals.data.from_ms !== windowMs
      ? undefined
      : totals.data;
  const scope = formatSpan(
    selection ? selection.toMs - selection.fromMs : windowMs
  );
  const measured = shown ? totalsMeasured(shown) : null;
  const unit = measured === null ? undefined : `in ${measured}`;
  return (
    <div data-testid="network-totals">
      <StatStrip
        items={[
          {
            label: `↓ Downloaded · ${scope}`,
            value: shown ? formatBytes(shown.rx_bytes) : MISSING,
            unit,
          },
          {
            label: `↑ Uploaded · ${scope}`,
            value: shown ? formatBytes(shown.tx_bytes) : MISSING,
            unit,
          },
        ]}
      />
    </div>
  );
}
