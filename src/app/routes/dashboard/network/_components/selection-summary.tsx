import { USAGE_KEEP_MS } from "@core/generated/bindings";
import { SelectionSummary as Summary } from "~/components/selection-summary";
import { useBrushRange } from "~/stores/brush-store";
import { useModuleRange } from "~/stores/chart-range";
import { useNetworkByApp } from "../_hooks/use-network-by-app";

/**
 * The shared summary line, naming the range the per-app answer covers:
 * history older than the 10 s tier widens a selection to its buckets.
 */
export function SelectionSummary({ windowMs }: { windowMs: number }) {
  const range = useBrushRange();
  const historical =
    useModuleRange()?.endMs != null || windowMs > USAGE_KEEP_MS;
  const { data } = useNetworkByApp(historical ? null : range);
  return (
    <Summary
      windowMs={windowMs}
      shown={data ? { fromMs: data.from_ms, toMs: data.to_ms } : null}
    />
  );
}
