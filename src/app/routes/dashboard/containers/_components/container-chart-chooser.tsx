import type { DockerSnapshot } from "@core/generated/bindings";
import type { useContainerComparison } from "../_hooks/use-container-comparison";
import { ChartChooser } from "./chart-chooser";
export function ContainerChartChooser({
  chart,
  snapshot,
  metric,
  open,
  onOpenChange,
  set,
}: {
  chart: ReturnType<typeof useContainerComparison>;
  snapshot: DockerSnapshot;
  metric: "cpu" | "memory";
  open: boolean;
  onOpenChange: (open: boolean) => void;
  set: (key: string, value: string | null) => void;
}) {
  const { candidates, slots, series, selected } = chart;
  return (
    <ChartChooser
      open={open}
      onOpenChange={onOpenChange}
      candidates={
        open
          ? [
              ...new Map([
                ...candidates.map((row) => [row.id, row.container] as const),
                ...series.map(
                  (line) => [line.container.id, line.container] as const
                ),
              ]).values(),
            ]
          : []
      }
      liveRows={snapshot.containers}
      selected={selected ?? []}
      slots={slots}
      metric={metric}
      onChange={(ids) => {
        const nextSlots = Object.fromEntries(
          Object.entries(slots).filter(([id]) => ids.includes(id))
        );
        for (const id of ids)
          if (!(id in nextSlots))
            nextSlots[id] =
              [1, 2, 3, 4, 5, 6].find(
                (slot) => !Object.values(nextSlots).includes(slot)
              ) ?? 6;
        set(
          "chart",
          [...ids]
            .sort((a, b) => (nextSlots[a] ?? 0) - (nextSlots[b] ?? 0))
            .join(",")
        );
      }}
      onAutomatic={() => {
        chart.automatic();
        set("chart", null);
      }}
    />
  );
}
