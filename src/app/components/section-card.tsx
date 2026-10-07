import { type ReactNode, useId } from "react";
import { cn } from "~/lib/utils";
import { Card } from "~/widgets/card";
import type { Accent, Corner } from "~/widgets/lib/accent";

export interface SectionCardProps {
  accent: Accent;
  origin?: Corner;
  /** Sentence case, names the measurement ("Battery, last 24 hours"). */
  title: string;
  /** Right side of the header: a figure, a note or a stat strip. */
  aside?: ReactNode;
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
  aside,
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
      className={cn("flex min-w-0 flex-col gap-3.5 p-4", className)}
    >
      <div className="flex flex-wrap items-start gap-x-6 gap-y-2">
        <h2 id={titleId} className="min-w-48 flex-1 font-[590] text-[14px]">
          {title}
        </h2>
        {aside}
      </div>
      {children}
    </Card>
  );
}
