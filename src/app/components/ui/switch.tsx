import { Switch as SwitchPrimitive } from "radix-ui";
import type * as React from "react";

import { cn } from "~/lib/utils";

// 30x18 track with a 12px thumb. Off is a faint track with a
// muted thumb; on is solid primary with a dark thumb.
function Switch({
  className,
  ...props
}: React.ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      data-slot="switch"
      className={cn(
        "group/switch peer inline-flex h-[18px] w-[30px] shrink-0 items-center rounded-full border border-border-strong bg-track outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:border-primary data-[state=checked]:bg-primary",
        className
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb
        data-slot="switch-thumb"
        className="pointer-events-none block size-3 rounded-full bg-muted-foreground ring-0 transition-[translate,scale] group-active/switch:scale-90 data-[state=checked]:translate-x-3.5 data-[state=unchecked]:translate-x-0.5 data-[state=checked]:bg-primary-foreground"
      />
    </SwitchPrimitive.Root>
  );
}

export { Switch };
