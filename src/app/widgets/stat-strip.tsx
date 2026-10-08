import { NumberTicker } from "~/lib/motion/number-ticker";
import { cn } from "~/lib/utils";
import { Swatch, type SwatchStep } from "./legend";
import { type Accent, accentVars } from "./lib/accent";
import { FIELD_LABEL } from "./lib/classes";

export interface StatStripItem {
  label: string;
  value: string;
  /** Muted suffix ("of 72.6 Wh"). */
  unit?: string;
  /** Muted trailing figures after the value ("2.98 2.71" for load average). */
  secondary?: string;
  /** Dim the value (Idle on the CPU page). */
  muted?: boolean;
  swatch?: SwatchStep;
}

export interface StatStripProps {
  /** The page's headline figure at hero size (CPU "Total 18%"). */
  hero?: { label: string; value: string; unit?: string };
  items: StatStripItem[];
  /** Defaults to the accent in scope; only used by swatches. */
  accent?: Accent;
}

/** Page-level KPI strip above a chart. */
export function StatStrip({ hero, items, accent }: StatStripProps) {
  return (
    <dl
      className={cn(
        "flex flex-wrap items-end",
        hero ? "gap-x-7 gap-y-3" : "gap-x-6 gap-y-3"
      )}
      style={accent ? accentVars(accent) : undefined}
    >
      {hero && (
        <div className="flex flex-col gap-0.5">
          <dt className={FIELD_LABEL}>{hero.label}</dt>
          <dd className="figures-display text-[32px] leading-none tracking-[-0.022em]">
            <NumberTicker text={hero.value} unit={hero.unit} />
            {hero.unit && (
              <span className="text-[15px] text-muted-foreground">
                {" "}
                {hero.unit}
              </span>
            )}
          </dd>
        </div>
      )}
      {items.map((item) => (
        <div key={item.label} className="flex flex-col gap-0.5">
          <dt className={cn(FIELD_LABEL, "flex items-center gap-1.5")}>
            {item.swatch && <Swatch step={item.swatch} size={7} />}
            {item.label}
          </dt>
          <dd
            className={cn(
              "figures-display text-[15px]",
              item.muted && "text-muted-foreground"
            )}
          >
            <NumberTicker text={item.value} unit={item.unit} />
            {item.secondary && (
              <span className="text-muted-foreground"> {item.secondary}</span>
            )}
            {item.unit && (
              <span className="text-[11px] text-muted-foreground">
                {" "}
                {item.unit}
              </span>
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}
