import { cva, type VariantProps } from "class-variance-authority";
import { Toggle as TogglePrimitive } from "radix-ui";
import type * as React from "react";
import { cn } from "~/lib/utils";

// 11px, muted until pressed, then a 14% primary
// tint with cyan text. `outline` is the stock bordered toggle.
const toggleVariants = cva(
  "inline-flex items-center justify-center gap-1.5 whitespace-nowrap outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:pointer-events-none disabled:opacity-50 [&_svg:not([class*='size-'])]:size-3.5 [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        default:
          "rounded-full text-muted-foreground hover:text-foreground data-[state=on]:bg-primary/14 data-[state=on]:text-cpu-2-ink",
        outline:
          "rounded-control border border-border bg-btn text-fg-subtle hover:bg-selected data-[state=on]:bg-selected data-[state=on]:text-foreground",
      },
      size: {
        default: "h-6 px-2.5 text-[11px]",
        sm: "h-[22px] px-2.5 text-[11px]",
        lg: "h-7 px-3 text-xs",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
);

function Toggle({
  className,
  variant,
  size,
  ...props
}: React.ComponentProps<typeof TogglePrimitive.Root> &
  VariantProps<typeof toggleVariants>) {
  return (
    <TogglePrimitive.Root
      data-slot="toggle"
      className={cn(toggleVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Toggle, toggleVariants };
