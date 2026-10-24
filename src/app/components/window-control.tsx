import {
  chartRangeLabel,
  RANGE_PRESETS,
  relativeWindow,
} from "@core/chart-range";
import type { ChartWindow } from "@core/generated/bindings";
import { gridIntervalMs } from "@core/live-state";
import { windowAllowed } from "@core/live-window";
import { Check, ChevronDown } from "lucide-react";
import { Popover } from "radix-ui";
import { useState } from "react";
import { SegmentedControl } from "~/components/segmented-control";
import { CustomRangeForm } from "~/components/window-control-custom";
import { useChartWindow, useSetChartWindow } from "~/hooks/use-chart-window";
import { cn } from "~/lib/utils";
import { useHost } from "~/stores/host-store";

/** Shared module range picker: two shortcuts and retained/custom ranges. */
export function WindowControl() {
  const intervalMs = useHost((s) => gridIntervalMs(s.status));
  const current = useChartWindow();
  const setWindow = useSetChartWindow();
  const [open, setOpen] = useState(false);
  if (current === null) return null;
  const quick =
    current.endMs === null && [300_000, 3_600_000].includes(current.windowMs);
  const apply = (value: ChartWindow) => {
    setWindow(value);
    setOpen(false);
  };
  return (
    <div className="flex items-center gap-1.5">
      <SegmentedControl
        options={[
          {
            value: "5m",
            label: "5m",
            disabled: !windowAllowed(300_000, intervalMs),
          },
          { value: "1h", label: "1h" },
        ]}
        value={typeof current.window === "string" ? current.window : ""}
        onChange={(value) => apply(value as ChartWindow)}
        ariaLabel="Window"
      />
      <Popover.Root open={open} onOpenChange={setOpen}>
        <Popover.Trigger asChild>
          <button
            type="button"
            aria-label={`Time range: ${chartRangeLabel(current.window)}`}
            className={cn(
              "figures inline-flex h-6 items-center gap-1.5 whitespace-nowrap rounded-full border px-2.5 text-[11px] outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring",
              !quick && "bg-primary/14 text-cpu-2-ink"
            )}
          >
            {quick ? "More" : chartRangeLabel(current.window)}
            <ChevronDown aria-hidden className="size-3" />
          </button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content
            align="end"
            sideOffset={6}
            collisionPadding={12}
            aria-label="Choose time range"
            className="z-50 max-h-(--radix-popover-content-available-height) w-96 max-w-[calc(100vw-24px)] overflow-y-auto rounded-card border bg-popover p-3 text-[12px] text-popover-foreground shadow-md outline-none dark:shadow-none"
          >
            <fieldset
              className="grid grid-cols-2 gap-x-2 gap-y-0.5"
              aria-label="Preset time ranges"
            >
              {RANGE_PRESETS.map((preset) => (
                <button
                  key={preset.ms}
                  type="button"
                  disabled={!windowAllowed(preset.ms, intervalMs)}
                  aria-pressed={
                    current.endMs === null && current.windowMs === preset.ms
                  }
                  onClick={() => apply(relativeWindow(preset.ms))}
                  className="flex items-center justify-between gap-2 rounded-sm px-2 py-1.5 text-left outline-none hover:bg-muted focus-visible:bg-muted focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-40 aria-pressed:bg-primary/14 aria-pressed:text-cpu-2-ink"
                >
                  {preset.label}
                  {current.endMs === null && current.windowMs === preset.ms && (
                    <Check className="size-3" aria-hidden />
                  )}
                </button>
              ))}
            </fieldset>
            <div className="mt-3 border-t pt-3">
              <CustomRangeForm
                value={current.window}
                intervalMs={intervalMs}
                onApply={apply}
              />
            </div>
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    </div>
  );
}
