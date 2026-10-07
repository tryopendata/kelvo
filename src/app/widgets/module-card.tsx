import { ArrowUpRight } from "lucide-react";
import { type ReactNode, useId } from "react";
import { cn } from "~/lib/utils";
import { LinkCard } from "./card";
import { CardNotice } from "./card-notice";
import type { Accent, Corner } from "./lib/accent";
import { FIELD_LABEL } from "./lib/classes";

export interface ModuleCardProps {
  accent: Accent;
  title: string;
  /** Headline figure on the right ("18%", "17.6"). */
  value?: string;
  /** Muted suffix after the value (" / 24 GB", " system"). */
  unit?: string;
  /**
   * Shown on the right instead of a value: muted text ("Wi‑Fi · en0") or a
   * mono field label ("% LOAD").
   */
  subtitle?: string;
  subtitleStyle?: "text" | "label";
  /** Popover cards share one light source (design-system.md): top left. */
  origin?: Corner;
  /** A state line under the header ("Sensor read failed · last value 11:02"). */
  notice?: string;
  /** Dashboard page the card opens. Without it the card is not interactive. */
  href?: string;
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
  title,
  value,
  unit,
  subtitle,
  subtitleStyle = "text",
  origin = "tl",
  notice,
  href,
  onOpen,
  children,
}: ModuleCardProps) {
  const titleId = useId();
  const content = (
    <>
      <header className="flex items-baseline justify-between gap-2">
        <h3
          id={titleId}
          className="flex items-center gap-1 font-[590] text-[12px]"
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
          <span className="data-mono text-[15px]">
            {value}
            {unit && (
              <span className="text-[12px] text-muted-foreground">{unit}</span>
            )}
          </span>
        ) : subtitle !== undefined ? (
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
      {notice && <CardNotice text={notice} />}
      {children}
    </>
  );
  return (
    <LinkCard
      accent={accent}
      origin={origin}
      variant="chart"
      labelledBy={titleId}
      href={href}
      onOpen={onOpen}
      className={cn(
        "flex flex-col gap-2 p-3",
        // No focus ring: `.vt-card` focus-visible already lifts the glow and
        // border, and WebKit keeps a ring on the card after it is clicked.
        href !== undefined && "group text-inherit no-underline outline-none"
      )}
    >
      {content}
    </LinkCard>
  );
}
