import { rangeFractions } from "@core/brush";
import { seriesRange } from "@core/chart-math";
import { gapExplanation } from "@core/history-state";
import type { ReactNode } from "react";
import { BrushOverlay } from "~/components/brush-overlay";
import { useChartBrush } from "~/hooks/use-chart-brush";
import type { WindowSeries } from "~/hooks/use-window-series";
import { useBrushStoreOptional } from "~/stores/brush-store";
import type { GapSpan } from "~/widgets/gap-band";

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
  enabled = true,
  gaps: readonly GapSpan[] = []
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
  const { fromMs: firstMs, toMs } = seriesRange(series);
  const spanMs = toMs - firstMs;
  return {
    highlight: selected ? rangeFractions(selected, firstMs, spanMs) : null,
    overlay: (
      <BrushOverlay
        firstMs={firstMs}
        spanMs={spanMs}
        selectableToMs={elapsedEdge ?? undefined}
        height={height}
        inset={0}
        gapExplanationAt={(time) =>
          gapExplanation(
            gaps.find((gap) => gap.fromMs <= time && gap.toMs > time)
          )
        }
      />
    ),
  };
}
