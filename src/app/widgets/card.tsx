import { Slot } from "radix-ui";
import type { CSSProperties, MouseEvent, ReactNode } from "react";
import { cn } from "~/lib/utils";
import { type Accent, accentVars, type Corner } from "./lib/accent";

export type CardProps = {
  accent: Accent;
  /** Glow origin. Defaults to top-left. */
  origin?: Corner;
  /** `chart` drops the glow tint to 6% (chart cards and every popover card). */
  variant?: "default" | "chart";
  /**
   * Render the single child (an `<a>`) as the card instead of a
   * `<section>`, merging the card's props onto it.
   */
  asChild?: boolean;
  className?: string;
  children?: ReactNode;
} & (
  | {
      /** Id of the element that names the card (its title). */
      labelledBy: string;
      ariaLabel?: never;
    }
  | {
      /** The card's name, when no element in it names it. */
      ariaLabel: string;
      labelledBy?: never;
    }
);

/**
 * Glow card shell (design-system.md "Card anatomy"): a `<section>` named by its
 * title, `--radius-card`, 1 px border and the corner glow in the module accent.
 */
export function Card({
  accent,
  origin = "tl",
  variant = "default",
  labelledBy,
  ariaLabel,
  asChild = false,
  className,
  children,
}: CardProps) {
  const Comp = asChild ? Slot.Root : "section";
  return (
    <Comp
      aria-labelledby={labelledBy}
      aria-label={ariaLabel}
      className={cn(
        "vt-card",
        variant === "chart" && "vt-card--chart",
        className
      )}
      style={accentVars(accent, origin) as CSSProperties}
    >
      {children}
    </Comp>
  );
}

export type LinkCardProps = CardProps & {
  /** Page the card opens. Without it the card is a plain, non-interactive card. */
  href?: string;
  /** Called instead of following `href` (the route navigates in-app). */
  onOpen?: (href: string) => void;
};

/** A card that, given `href`, is itself the link to the page it opens. */
export function LinkCard({ href, onOpen, children, ...card }: LinkCardProps) {
  if (href === undefined) return <Card {...card}>{children}</Card>;
  const onClick = onOpen
    ? (event: MouseEvent<HTMLAnchorElement>) => {
        event.preventDefault();
        onOpen(href);
      }
    : undefined;
  return (
    <Card {...card} asChild>
      <a href={href} onClick={onClick}>
        {children}
      </a>
    </Card>
  );
}
