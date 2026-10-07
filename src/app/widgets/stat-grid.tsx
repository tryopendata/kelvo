import { cn } from "~/lib/utils";
import { Swatch, type SwatchStep } from "./legend";
import { type Accent, accentVars } from "./lib/accent";
import { FIELD_LABEL } from "./lib/classes";

export interface StatGridItem {
  label: string;
  value: string;
  unit?: string;
  /** Legend swatch before the label (popover power: CPU, GPU, ANE, DRAM). */
  step?: SwatchStep;
  /** Dim the value (a series reading zero, like ANE at rest). */
  muted?: boolean;
}

export interface StatGridProps {
  items: StatGridItem[];
  /** Defaults to the accent in scope; only used by swatches. */
  accent?: Accent;
}

/** Two to four mono KPI cells with field labels (GPU, battery, power). */
export function StatGrid({ items, accent }: StatGridProps) {
  return (
    <dl
      className="grid gap-2"
      style={{
        ...(accent ? accentVars(accent) : {}),
        gridTemplateColumns: `repeat(${items.length}, minmax(0, 1fr))`,
      }}
    >
      {items.map((item) => (
        <div key={item.label} className="flex flex-col gap-0.5">
          <dt className={cn(FIELD_LABEL, "flex items-center gap-1.5")}>
            {item.step && <Swatch step={item.step} size={7} />}
            {item.label}
          </dt>
          <dd
            className={cn(
              "data-mono text-[12px]",
              item.muted && "text-muted-foreground"
            )}
          >
            {item.value}
            {item.unit && (
              <span className="text-muted-foreground"> {item.unit}</span>
            )}
          </dd>
        </div>
      ))}
    </dl>
  );
}
