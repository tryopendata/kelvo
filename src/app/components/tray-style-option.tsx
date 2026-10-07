import { cn } from "~/lib/utils";
import { TrayPreview, type TrayStyle, type TrayValues } from "./tray-preview";

export interface TrayStyleOptionProps {
  style: TrayStyle;
  title: string;
  /** One sentence on what the style costs or gives ("One item. Smallest footprint."). */
  description: string;
  recommended?: boolean;
  selected: boolean;
  values: TrayValues;
  onSelect: (style: TrayStyle) => void;
}

/**
 * Selectable menu bar style card with a live TrayPreview. A
 * radio: render the options inside a `role="radiogroup"`.
 */
export function TrayStyleOption({
  style,
  title,
  description,
  recommended = false,
  selected,
  values,
  onSelect,
}: TrayStyleOptionProps) {
  return (
    // biome-ignore lint/a11y/useSemanticElements: a card-sized radio with a preview inside; a native radio input cannot hold it
    <button
      type="button"
      role="radio"
      aria-checked={selected}
      onClick={() => onSelect(style)}
      className={cn(
        "flex w-full flex-col gap-2 rounded-tile border border-border bg-btn p-3 text-left text-foreground outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
        selected ? "border-primary bg-primary/6" : "hover:bg-selected"
      )}
    >
      <span className="flex w-full items-baseline justify-between">
        <span className="text-[13px]">{title}</span>
        {recommended && (
          <span className="font-normal text-[11px] text-link">Recommended</span>
        )}
      </span>
      <TrayPreview style={style} values={values} className="w-full" />
      <span className="font-normal text-[11px] text-muted-foreground">
        {description}
      </span>
    </button>
  );
}
