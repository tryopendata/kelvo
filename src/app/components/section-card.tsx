import { type ReactNode, useId } from "react";
import { cn } from "~/lib/utils";
import { Card } from "~/widgets/card";
import type { Accent, Corner } from "~/widgets/lib/accent";

const ALIGN = {
  start: "items-start",
  center: "items-center",
  baseline: "items-baseline",
} as const;

export interface SectionCardProps {
  accent: Accent;
  origin?: Corner;
  /** Sentence case, names the measurement ("Battery, last 24 hours"). */
  title: string;
  /**
   * Keep the title for assistive tech only: the card's figures (a stat
   * strip) already say what it is. No header row is drawn.
   */
  hiddenTitle?: boolean;
  /** Right side of the header: a figure, a note or a stat strip. */
  aside?: ReactNode;
  /** How the title and `aside` line up. */
  headerAlign?: keyof typeof ALIGN;
  /**
   * No card padding or gap: the header carries its own padding and the
   * body (a table) runs edge to edge.
   */
  flush?: boolean;
  variant?: "default" | "chart";
  className?: string;
  children?: ReactNode;
}

/**
 * Dashboard section card: 16 px padding, 14 px title at 590,
 * an optional right-hand header slot, 14 px between header and body.
 */
export function SectionCard({
  accent,
  origin,
  title,
  hiddenTitle = false,
  aside,
  headerAlign = "start",
  flush = false,
  variant = "chart",
  className,
  children,
}: SectionCardProps) {
  const titleId = useId();
  return (
    <Card
      accent={accent}
      origin={origin}
      variant={variant}
      labelledBy={titleId}
      className={cn(
        "flex min-w-0 flex-col",
        !flush && "gap-3.5 p-4",
        className
      )}
    >
      {hiddenTitle ? (
        <h2 id={titleId} className="sr-only">
          {title}
        </h2>
      ) : (
        <div
          className={cn(
            "flex flex-wrap gap-x-6 gap-y-2",
            ALIGN[headerAlign],
            flush && "min-h-6 px-4 pt-3.5 pb-2"
          )}
        >
          <h2 id={titleId} className="min-w-48 flex-1 font-[590] text-[14px]">
            {title}
          </h2>
          {aside}
        </div>
      )}
      {children}
    </Card>
  );
}
