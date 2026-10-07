import { useLayoutEffect, useRef } from "react";
import { ToggleGroup, ToggleGroupItem } from "~/components/ui/toggle-group";
import { cn } from "~/lib/utils";

export interface SegmentedOption<V extends string> {
  value: V;
  label: string;
  /** Shown but not selectable. */
  disabled?: boolean;
}

export interface SegmentedControlProps<V extends string> {
  options: readonly SegmentedOption<V>[];
  value: V;
  onChange: (value: V) => void;
  /** `sm` is a 22 px pill; `md` is 24 px. */
  size?: "sm" | "md";
  ariaLabel: string;
  className?: string;
}

/**
 * Pill track with one selected option. The
 * selected tint is a single thumb that slides between options over
 * `--motion-fast`; it is instant under reduced motion.
 * Selecting the current option again does nothing: there is always a value.
 */
export function SegmentedControl<V extends string>({
  options,
  value,
  onChange,
  size = "sm",
  ariaLabel,
  className,
}: SegmentedControlProps<V>) {
  const rootRef = useRef<HTMLDivElement>(null);
  const thumbRef = useRef<HTMLSpanElement>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: re-measure when the selection or the option labels change
  useLayoutEffect(() => {
    const root = rootRef.current;
    const thumb = thumbRef.current;
    if (!root || !thumb) return;
    const on = root.querySelector<HTMLElement>("[data-state=on]");
    if (!on) {
      thumb.style.opacity = "0";
      return;
    }
    thumb.style.opacity = "1";
    thumb.style.width = `${on.offsetWidth}px`;
    thumb.style.transform = `translateX(${on.offsetLeft - 2}px)`;
  }, [value, options]);

  return (
    <ToggleGroup
      ref={rootRef}
      type="single"
      size={size === "sm" ? "sm" : "default"}
      value={value}
      onValueChange={(next) => {
        if (next) onChange(next as V);
      }}
      aria-label={ariaLabel}
      className={cn("relative", className)}
    >
      <span
        ref={thumbRef}
        aria-hidden
        className="pointer-events-none absolute top-0.5 bottom-0.5 left-0.5 rounded-full bg-primary/14 opacity-0 transition-transform duration-(--motion-fast) ease-out"
      />
      {options.map((o) => (
        <ToggleGroupItem
          key={o.value}
          value={o.value}
          disabled={o.disabled}
          className="relative transition-[color,background-color,scale] active:scale-[0.96] data-[state=on]:bg-transparent data-[state=on]:active:scale-100"
        >
          {o.label}
        </ToggleGroupItem>
      ))}
    </ToggleGroup>
  );
}
