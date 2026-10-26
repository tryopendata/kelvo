import { ArrowUpRight, ChevronDown, ChevronRight } from "lucide-react";
import { type ReactNode, useId } from "react";
import { cn } from "~/lib/utils";
import { LinkCard } from "./card";
import { CardNotice } from "./card-notice";
import type { Accent, Corner } from "./lib/accent";
import { FIELD_LABEL } from "./lib/classes";

export interface ModuleCardProps {
  accent: Accent;
  compact?: boolean;
  onToggle?: (() => void) | undefined;
  title: string;
  /** Headline figure on the right ("18%", "17.6"). */
  value?: string | undefined;
  /** Muted suffix after the value (" / 24 GB", " system"). */
  unit?: string | undefined;
  /**
   * Shown on the right instead of a value: muted text ("Wi‑Fi · en0") or a
   * field label ("% load").
   */
  subtitle?: string | undefined;
  subtitleStyle?: "text" | "label";
  /**
   * Controls in the subtitle's place (copyable addresses). Drawn over the
   * card, outside its link, so a click on them never opens the page and no
   * button nests in the anchor.
   */
  aside?: ReactNode;
  /** Popover cards share one light source (design-system.md): top left. */
  origin?: Corner;
  /** A state line under the header ("Sensor read failed · last value 11:02"). */
  notice?: string | undefined;
  /** Dashboard page the card opens. Without it the card is not interactive. */
  href?: string | undefined;
  /** Called instead of following `href` (the popover opens the dashboard). */
  onOpen?: (href: string) => void;
  children?: ReactNode;
}

/**
 * Popover module card: title left, headline value right,
 * body slot below. 12 px padding, 6% glow. With `href` the whole card is the
 * link to its dashboard page.
 */
export function ModuleCard({
  accent,
  compact = false,
  onToggle,
  title,
  value,
  unit,
  subtitle,
  subtitleStyle = "text",
  aside,
  origin = "tl",
  notice,
  href,
  onOpen,
  children,
}: ModuleCardProps) {
  const titleId = useId();
  const content = (
    <>
      <ModuleCardHeader
        titleId={titleId}
        title={title}
        href={href}
        compact={compact}
        onToggle={onToggle}
        value={value}
        unit={unit}
        subtitle={subtitle}
        subtitleStyle={subtitleStyle}
        aside={aside}
      />
      {notice && <CardNotice text={notice} />}
      {children}
    </>
  );
  const card = (
    <LinkCard
      accent={accent}
      origin={origin}
      variant="chart"
      labelledBy={titleId}
      href={href}
      onOpen={onOpen}
      className={cn(
        "flex flex-col",
        compact ? "min-h-14 gap-1 p-2" : "gap-2 p-3",
        // No focus ring: Card focus-visible already lifts the glow and
        // border, and WebKit keeps a ring on the card after it is clicked.
        href !== undefined && "group text-inherit no-underline outline-none"
      )}
    >
      {content}
    </LinkCard>
  );
  if (aside === undefined && !onToggle) return card;
  return (
    <div className="relative">
      {card}
      {onToggle && (
        <button
          type="button"
          aria-label={`${compact ? "Expand" : "Collapse"} ${title}`}
          aria-expanded={!compact}
          onClick={(event) => {
            event.currentTarget.focus({ preventScroll: true });
            onToggle();
          }}
          className={cn(
            "absolute left-2 flex size-5 items-center justify-center rounded-control text-muted-foreground outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring",
            compact ? "top-2" : "top-3"
          )}
        >
          {compact ? (
            <ChevronRight aria-hidden className="size-3.5" />
          ) : (
            <ChevronDown aria-hidden className="size-3.5" />
          )}
        </button>
      )}
      {aside !== undefined && (
        <div className="absolute top-3 right-3 font-normal text-[11px] text-muted-foreground">
          {aside}
        </div>
      )}
    </div>
  );
}

function ModuleCardHeader({
  titleId,
  title,
  href,
  compact,
  onToggle,
  value,
  unit,
  subtitle,
  subtitleStyle,
  aside,
}: Pick<
  ModuleCardProps,
  | "title"
  | "href"
  | "compact"
  | "onToggle"
  | "value"
  | "unit"
  | "subtitle"
  | "subtitleStyle"
  | "aside"
> & { titleId: string }) {
  return (
    <header className="flex items-baseline justify-between gap-2">
      <h3
        id={titleId}
        className={cn(
          "flex items-center gap-1 font-[590] text-[12px]",
          onToggle && "pl-5"
        )}
      >
        {title}
        {href !== undefined && (
          // Shown on hover and focus: the card opens a page in another window.
          <ArrowUpRight
            aria-hidden
            strokeWidth={2}
            className="size-3 -translate-x-0.5 text-muted-foreground opacity-0 transition-[opacity,translate] duration-(--motion-fast) ease-(--ease-out) group-hover:translate-x-0 group-hover:opacity-100 group-focus-visible:translate-x-0 group-focus-visible:opacity-100"
          />
        )}
      </h3>
      {value !== undefined ? (
        <span
          className={cn(
            "figures-display whitespace-nowrap",
            compact ? "text-[12px]" : "text-[15px]"
          )}
        >
          {value}
          {unit && (
            <span className="text-[12px] text-muted-foreground">{unit}</span>
          )}
        </span>
      ) : subtitle !== undefined && aside === undefined ? (
        <span
          className={
            subtitleStyle === "label"
              ? FIELD_LABEL
              : "font-normal text-[11px] text-muted-foreground"
          }
        >
          {subtitle}
        </span>
      ) : null}
    </header>
  );
}
