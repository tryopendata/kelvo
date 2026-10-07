import type { CSSProperties, ReactNode, Ref } from "react";
import { cn } from "~/lib/utils";

export interface ChartTooltipShellProps {
  ref?: Ref<HTMLDivElement>;
  hidden?: boolean;
  /** Width and placement ("top-0 w-[220px]"). */
  className?: string;
  style?: CSSProperties;
  children: ReactNode;
}

/**
 * The frosted card a chart's hover readout sits in: the Timeline crosshair
 * tooltip and the live mirror chart's hover. Markup only; each caller fills
 * and places it its own way.
 */
export function ChartTooltipShell({
  ref,
  hidden,
  className,
  style,
  children,
}: ChartTooltipShellProps) {
  return (
    <div
      ref={ref}
      hidden={hidden}
      role="tooltip"
      className={cn(
        "pointer-events-none absolute z-10 flex flex-col gap-2 rounded-tile border border-border-strong bg-card/92 p-3 backdrop-blur-md",
        className
      )}
      style={style}
    >
      {children}
    </div>
  );
}

/** The 8 px color mark in front of a series row. */
export function SeriesSwatch({ color }: { color: string }) {
  return (
    <span
      aria-hidden
      className="size-2 rounded-mark"
      style={{ background: color }}
    />
  );
}
