import { ScrollArea } from "radix-ui";
import type { ReactNode } from "react";
import { cn } from "~/lib/utils";

/**
 * The popover's card column: an overlay scrollbar that shows
 * while scrolling and takes no layout width, and a callback for the header
 * border once the column has left the top.
 */
export function PopoverScroll({
  onScrolledChange,
  className,
  children,
}: {
  onScrolledChange: (scrolled: boolean) => void;
  className?: string;
  children: ReactNode;
}) {
  return (
    <ScrollArea.Root type="scroll" className="relative min-h-0 flex-1">
      <ScrollArea.Viewport
        aria-label="Modules"
        className="size-full outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
        onScroll={(e) => onScrolledChange(e.currentTarget.scrollTop > 0)}
      >
        <div className={cn("flex flex-col gap-2 px-2.5 pb-2.5", className)}>
          {children}
        </div>
      </ScrollArea.Viewport>
      <ScrollArea.Scrollbar
        orientation="vertical"
        className="flex w-2 touch-none select-none p-0.5"
      >
        <ScrollArea.Thumb className="relative flex-1 rounded-full bg-scrollbar" />
      </ScrollArea.Scrollbar>
    </ScrollArea.Root>
  );
}
