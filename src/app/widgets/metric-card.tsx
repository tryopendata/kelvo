import { useId } from "react";
import { cn } from "~/lib/utils";
import { LinkCard } from "./card";
import { CardNotice } from "./card-notice";
import { InlineBar, type InlineBarProps } from "./inline-bar";
import { Legend, type LegendItem } from "./legend";
import type { Accent, Corner } from "./lib/accent";
import { ProcessList, type ProcessListRow } from "./process-list";
import { RingGauge, type RingGaugeProps } from "./ring-gauge";
import { StreamArea, type StreamAreaProps } from "./stream-area";

/**
 * What fills the bottom of the card: a top-5 list (processes, or interfaces on
 * the Network card without per-process network), or a 60 s chart (the GPU card
 * on a host without `process_gpu`). `note` is one quiet line under
 * the list saying what it covers ("Your processes only").
 */
export type MetricCardBody =
  | { kind: "list"; ariaLabel: string; rows: ProcessListRow[]; note?: string }
  | { kind: "stream"; stream: StreamAreaProps };

export interface MetricCardProps {
  accent: Accent;
  origin?: Corner;
  title: string;
  /** Static host fact on the right ("10P + 4E", "on battery"). */
  subtitle: string;
  ring: Omit<RingGaugeProps, "size" | "accent">;
  bars: [
    Omit<InlineBarProps, "layout" | "accent">,
    Omit<InlineBarProps, "layout" | "accent">,
  ];
  legend: LegendItem[];
  body: MetricCardBody;
  /** A state line under the header ("Sensor read failed · last value 11:02"). */
  notice?: string;
  /** Module page the card opens. Without it the card is not interactive. */
  href?: string;
  /** Called instead of following `href` (the route navigates in-app). */
  onOpen?: (href: string) => void;
}

/**
 * Overview module card: header, ring, two bars, legend and a
 * top-5 list or chart. The whole card is the link to its module page.
 */
export function MetricCard({
  accent,
  origin = "tl",
  title,
  subtitle,
  ring,
  bars,
  legend,
  body,
  notice,
  href,
  onOpen,
}: MetricCardProps) {
  const titleId = useId();
  const content = (
    <>
      <header className="flex items-baseline justify-between gap-2">
        <div className="flex items-center gap-2">
          <span
            aria-hidden
            className="size-2 rounded-mark"
            style={{ background: "var(--a)" }}
          />
          <h3 id={titleId} className="font-[590] text-[13px]">
            {title}
          </h3>
        </div>
        <span className="font-normal text-[11px] text-muted-foreground">
          {subtitle}
        </span>
      </header>
      {notice && <CardNotice text={notice} />}
      <div className="grid grid-cols-[72px_minmax(0,1fr)] items-center gap-4">
        <RingGauge {...ring} accent={accent} size={72} />
        <div className="flex min-w-0 flex-col gap-[9px]">
          {bars.map((bar) => (
            <InlineBar key={bar.label} {...bar} layout="stacked" />
          ))}
        </div>
      </div>
      <Legend items={legend} />
      <div className="border-border-subtle border-t pt-[9px]">
        {body.kind === "list" ? (
          <>
            <ProcessList rows={body.rows} ariaLabel={body.ariaLabel} />
            {body.note && (
              <p className="mt-1.5 font-normal text-[11px] text-muted-foreground">
                {body.note}
              </p>
            )}
          </>
        ) : (
          <StreamArea {...body.stream} />
        )}
      </div>
    </>
  );

  return (
    <LinkCard
      accent={accent}
      origin={origin}
      labelledBy={titleId}
      href={href}
      onOpen={onOpen}
      className={cn(
        "flex flex-col gap-3 px-4 py-3.5",
        href !== undefined &&
          "text-inherit no-underline outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background"
      )}
    >
      {content}
    </LinkCard>
  );
}
