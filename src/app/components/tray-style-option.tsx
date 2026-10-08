import type { TrayStyle } from "@core/settings-patch";
import type { TrayLayout } from "@core/tray-layout";
import { RadioGroup } from "radix-ui";
import type { ComponentProps } from "react";
import { cn } from "~/lib/utils";
import { TrayPreview } from "./tray-preview";

export interface TrayStyleGroupProps
  extends Omit<
    ComponentProps<typeof RadioGroup.Root>,
    "value" | "defaultValue" | "onValueChange"
  > {
  value: TrayStyle;
  onValueChange: (style: TrayStyle) => void;
}

/**
 * The radio group around `TrayStyleOption` cards: one tab stop on the
 * checked card, arrow keys move between cards and select.
 */
export function TrayStyleGroup({
  value,
  onValueChange,
  ...props
}: TrayStyleGroupProps) {
  return (
    <RadioGroup.Root
      value={value}
      onValueChange={(v) => onValueChange(v as TrayStyle)}
      {...props}
    />
  );
}

export interface TrayStyleOptionProps {
  style: TrayStyle;
  title: string;
  /** One sentence on what the style costs or gives ("One item. Smallest footprint."). */
  description: string;
  recommended?: boolean;
  /** The style's preset drawn over the current values (`trayLayout`). */
  layout: TrayLayout;
}

/**
 * Selectable menu bar style card with a live TrayPreview. A radio: render
 * the options inside a `TrayStyleGroup`.
 */
export function TrayStyleOption({
  style,
  title,
  description,
  recommended = false,
  layout,
}: TrayStyleOptionProps) {
  return (
    <RadioGroup.Item
      value={style}
      className={cn(
        "flex w-full flex-col gap-2 rounded-tile border border-border bg-btn p-3 text-left text-foreground outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background",
        "data-[state=checked]:border-primary data-[state=checked]:bg-primary/6 data-[state=unchecked]:hover:bg-selected"
      )}
    >
      <span className="flex w-full items-baseline justify-between">
        <span className="text-[13px]">{title}</span>
        {recommended && (
          <span className="font-normal text-[11px] text-link">Recommended</span>
        )}
      </span>
      <TrayPreview layout={layout} className="w-full" />
      <span className="font-normal text-[11px] text-muted-foreground">
        {description}
      </span>
    </RadioGroup.Item>
  );
}
