import { SelectionSummary as Summary } from "~/components/selection-summary";
import { useBrushRange } from "~/stores/brush-store";
import { useNetworkByApp } from "../_hooks/use-network-by-app";

/**
 * The shared summary line, naming the range the per-app answer covers:
 * history older than the 10 s tier widens a selection to its buckets.
 */
export function SelectionSummary({ windowMs }: { windowMs: number }) {
  const range = useBrushRange();
  const { data } = useNetworkByApp(range);
  return (
    <Summary
      windowMs={windowMs}
      shown={data ? { fromMs: data.from_ms, toMs: data.to_ms } : null}
    />
  );
}
