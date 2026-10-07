import type { CSSProperties } from "react";
import { cn } from "~/lib/utils";

export interface WindowTicksProps {
  /** Labels from the oldest to "now", spread edge to edge (`windowTicks`). */
  ticks: readonly string[];
  className?: string;
  style?: CSSProperties;
}

/**
 * The x-axis label row under a live chart: faint labels, with the last one
 * ("now") brighter.
 */
export function WindowTicks({ ticks, className, style }: WindowTicksProps) {
  return (
    <div
      aria-hidden
      className={cn("flex justify-between", className)}
      style={style}
    >
      {ticks.map((t, i) => (
        <span
          // A window under 2 s gives two "−1s" labels; position is the identity.
          // biome-ignore lint/suspicious/noArrayIndexKey: fixed tick slots
          key={i}
          className={cn(
            "data-mono text-[10px]",
            i === ticks.length - 1 ? "text-muted-foreground" : "text-fg-faint"
          )}
        >
          {t}
        </span>
      ))}
    </div>
  );
}
