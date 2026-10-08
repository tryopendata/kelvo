import { useId } from "react";
import { Card } from "./card";
import type { Accent, Corner } from "./lib/accent";
import { FIELD_LABEL } from "./lib/classes";
import { RingGauge, type RingGaugeProps } from "./ring-gauge";

export interface RingStatCardProps {
  /** Card accent (the glow); the ring carries its own (temperature in amber). */
  accent: Accent;
  origin?: Corner;
  title: string;
  /** Omit when the page header already states it (Battery's "On battery"). */
  description?: string;
  /**
   * The ring, or `null` when there is nothing to gauge (passive cooling): the
   * description then carries the state.
   */
  ring: Omit<RingGaugeProps, "size"> | null;
  kv: { label: string; value: string };
}

/** Ring plus title, description and one key/value. */
export function RingStatCard({
  accent,
  origin,
  title,
  description,
  ring,
  kv,
}: RingStatCardProps) {
  const titleId = useId();
  return (
    <Card
      accent={accent}
      origin={origin}
      labelledBy={titleId}
      className="flex items-center gap-4 p-4"
    >
      {ring ? (
        <RingGauge {...ring} size={88} />
      ) : (
        <span aria-hidden className="size-[88px] shrink-0" />
      )}
      <div className="flex min-w-0 flex-col gap-1.5">
        <h3 id={titleId} className="font-[590] text-[13px]">
          {title}
        </h3>
        {description && (
          <span className="font-normal text-[11px] text-muted-foreground">
            {description}
          </span>
        )}
        <dl className="flex flex-col gap-px">
          <dt className={FIELD_LABEL}>{kv.label}</dt>
          <dd className="figures text-[13px]">{kv.value}</dd>
        </dl>
      </div>
    </Card>
  );
}
