import type { ReactNode } from "react";
import { cn } from "~/lib/utils";

/**
 * One gallery entry: the component name and its mock-props render. `width`
 * pins the frame to the size the component has where it is used.
 */
export function GalleryItem({
  name,
  usedIn,
  width,
  vibrant = false,
  children,
}: {
  name: string;
  /** Where the component appears, e.g. "Overview". */
  usedIn?: string;
  width?: number;
  /** Render inside a `.surface-vibrant` panel (popover and widget surfaces). */
  vibrant?: boolean;
  children: ReactNode;
}) {
  return (
    <figure
      data-gallery-item={name}
      className="flex min-w-0 flex-col gap-2"
      style={width ? { width } : undefined}
    >
      <figcaption className="flex items-baseline gap-2">
        <span className="font-[590] text-[13px]">{name}</span>
        {usedIn && (
          <span className="data-mono text-[11px] text-muted-foreground">
            {usedIn}
          </span>
        )}
      </figcaption>
      <div
        className={cn(
          "flex flex-col gap-2",
          vibrant && "surface-vibrant rounded-panel border border-border p-2.5"
        )}
      >
        {children}
      </div>
    </figure>
  );
}
