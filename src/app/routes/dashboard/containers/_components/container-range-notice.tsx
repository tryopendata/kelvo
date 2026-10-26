import { recordedDate } from "@core/chart-range";
import { useChartWindow } from "~/hooks/use-chart-window";

export function ContainerRangeNotice() {
  const range = useChartWindow();
  if (!range || range.endMs === null) return null;
  return (
    <p role="status" className="text-[11px] text-muted-foreground">
      History: {recordedDate(range.endMs - range.windowMs)} to{" "}
      {recordedDate(range.endMs)} ·{" "}
      {Intl.DateTimeFormat().resolvedOptions().timeZone}. Inventory and controls
      retain their labeled current or last-observed state.
    </p>
  );
}
