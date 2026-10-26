import { memo } from "react";
import { SegmentedControl } from "~/components/segmented-control";
import { type DockerRange, HistoryControls } from "./history-controls";

export const ComparisonControls = memo(function ComparisonControls({
  metric,
  range,
  end,
  now,
  windowMs,
  onMetric,
  onRange,
  onEnd,
}: {
  metric: "cpu" | "memory";
  range: DockerRange;
  end: number | null;
  now: number;
  windowMs?: number;
  onMetric: (value: "cpu" | "memory") => void;
  onRange: (value: DockerRange) => void;
  onEnd: (value: number | null) => void;
}) {
  return (
    <div className="flex flex-wrap items-center gap-3">
      <SegmentedControl
        options={[
          { value: "cpu", label: "CPU" },
          { value: "memory", label: "Memory" },
        ]}
        value={metric}
        onChange={onMetric}
        ariaLabel="Workload chart"
      />
      <HistoryControls
        range={range}
        windowMs={windowMs}
        end={end}
        now={now}
        onRange={onRange}
        onEnd={onEnd}
      />
    </div>
  );
});
