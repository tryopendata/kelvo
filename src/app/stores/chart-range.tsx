import type { TimeRange } from "@core/brush";
import { dateInput } from "@core/chart-range";
import { RING_SPAN_MS } from "@core/generated/bindings";
import {
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useRef,
} from "react";
import {
  type ChartWindowState,
  useChartWindow,
} from "~/hooks/use-chart-window";
import { BrushProvider, useBrushStore } from "~/stores/brush-store";

const ChartRangeContext = createContext<ChartWindowState | null>(null);
export const useModuleRange = () => useContext(ChartRangeContext);

/** The chosen range is shared by charts and summaries, clearing the brush when fixed date bounds change. */
export function ChartRangeProvider({
  children,
  initial = null,
}: {
  children: ReactNode;
  initial?: TimeRange | null;
}) {
  const range = useChartWindow();
  return (
    <ChartRangeContext.Provider value={range}>
      <BrushProvider initial={initial}>
        <ResetFixedBrush
          rangeKey={
            range
              ? range.endMs === null
                ? null
                : `${range.endMs}:${range.windowMs}`
              : undefined
          }
        />
        {children}
      </BrushProvider>
    </ChartRangeContext.Provider>
  );
}

export function ChartRangeNotice() {
  const range = useModuleRange();
  return (
    <>
      {range && (range.endMs !== null || range.windowMs > RING_SPAN_MS) && (
        <p
          className="font-normal text-[11px] text-muted-foreground"
          role="status"
        >
          {range.endMs !== null &&
            `${dateInput(range.endMs - range.windowMs)} to ${dateInput(range.endMs)} · ${Intl.DateTimeFormat().resolvedOptions().timeZone}. `}
          Charts and totals use recorded history. Current readings stay live.
          Hatched areas have missing data. App activity may cover only part of
          this range.
        </p>
      )}
    </>
  );
}

function ResetFixedBrush({
  rangeKey,
}: {
  rangeKey: string | null | undefined;
}) {
  const store = useBrushStore();
  const previous = useRef(rangeKey);
  useEffect(() => {
    if (previous.current !== undefined && previous.current !== rangeKey)
      store.getState().clear();
    previous.current = rangeKey;
  }, [rangeKey, store]);
  return null;
}
