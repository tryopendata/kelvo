import type { CSSProperties, ReactNode } from "react";
import { cn } from "~/lib/utils";
import { type Accent, accentVars, type Corner } from "./lib/accent";

export interface CardProps {
  accent: Accent;
  /** Glow origin. Defaults to top-left. */
  origin?: Corner;
  /** `chart` drops the glow tint to 6% (chart cards and every popover card). */
  variant?: "default" | "chart";
  /** Id of the element that names the card (its title). */
  labelledBy: string;
  className?: string;
  children?: ReactNode;
}

/**
 * Glow card shell (design-system.md "Card anatomy"): a `<section>` named by its
 * title, `--radius-card`, 1 px border and the corner glow in the module accent.
 */
export function Card({
  accent,
  origin = "tl",
  variant = "default",
  labelledBy,
  className,
  children,
}: CardProps) {
  return (
    <section
      aria-labelledby={labelledBy}
      className={cn(
        "vt-card",
        variant === "chart" && "vt-card--chart",
        className
      )}
      style={accentVars(accent, origin) as CSSProperties}
    >
      {children}
    </section>
  );
}
