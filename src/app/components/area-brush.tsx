import { rangeFractions } from "@core/brush";
import type { ReactNode } from "react";
import { BrushOverlay } from "~/components/brush-overlay";
import { useChartBrush } from "~/hooks/use-chart-brush";
import type { WindowSeries } from "~/hooks/use-window-series";
import { useBrushStoreOptional } from "~/stores/brush-store";

/**
 * The brush for a `StreamArea` (D-089, D-099): its `highlight` and its
 * `overlay`, both from the series it draws. The plot runs from the first
 * point to the last, so that is the span the overlay maps the pointer onto.
 * Read the series with `useWindowSeries(keys, windowMs, { brush: true })`,
 * so the points line up with 10 s buckets. Off outside a `BrushProvider`.
 */
export function useAreaBrush(
  series: WindowSeries,
  height: number,
  enabled = true
): {
  highlight: { left: number; width: number } | null;
  overlay: ReactNode;
} {
  const provider = useBrushStoreOptional();
  const on = enabled && provider !== null;
  const { selected, elapsedEdge } = useChartBrush(on);
  const n = Math.max(0, ...Object.values(series.values).map((v) => v.length));
  if (!on || series.tEndMs <= 0 || n < 2) {
    return { highlight: null, overlay: null };
  }
  const spanMs = (n - 1) * series.intervalMs;
  const firstMs = series.tEndMs - spanMs;
  return {
    highlight: selected ? rangeFractions(selected, firstMs, spanMs) : null,
    overlay: (
      <BrushOverlay
        firstMs={firstMs}
        spanMs={spanMs}
        selectableToMs={elapsedEdge ?? undefined}
        height={height}
        inset={0}
      />
    ),
  };
}
