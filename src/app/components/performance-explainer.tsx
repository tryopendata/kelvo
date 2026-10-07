import type { PerformanceReason } from "@core/generated/bindings";
import { performanceChanges, performanceWhy } from "@core/performance";
import { type ReactNode, useId } from "react";
import {
  HoverCard,
  HoverCardContent,
  HoverCardTrigger,
} from "~/components/ui/hover-card";
import { cn } from "~/lib/utils";
import { useSettings } from "~/stores/settings-store";

export interface PerformanceExplainerProps {
  /** Why the mode is on; nothing renders for `off`. */
  reason: PerformanceReason;
  /** Opens Settings > Sampling, where the mode is turned off. */
  onOpenSettings: () => void;
  /** The marker: "Performance mode" in the sidebar, the pill in the popover. */
  children: ReactNode;
  /** The marker's visible text; the button's name adds what a click does. */
  label: string;
  className?: string;
  side?: "top" | "right" | "bottom";
}

/**
 * A Performance mode marker that explains itself (D-088): hover or focus
 * shows why the mode is on and what it changed, from the same list as the
 * Settings disclosure. Screen readers get the same text as the description,
 * and the marker itself opens Settings, since keyboard focus can't reach the
 * card's link.
 */
export function PerformanceExplainer({
  reason,
  onOpenSettings,
  children,
  label,
  className,
  side = "top",
}: PerformanceExplainerProps) {
  const id = useId();
  const sampling = useSettings((s) => s.sampling);
  const modules = useSettings((s) => s.modules);
  const changes =
    sampling && modules
      ? performanceChanges({ sampling, modules }, reason)
      : [];
  const why = performanceWhy(reason);
  if (why === null) return null;

  return (
    <HoverCard>
      <HoverCardTrigger asChild>
        <button
          type="button"
          aria-label={`${label}, open Performance settings`}
          aria-describedby={id}
          onClick={onOpenSettings}
          className={cn(
            "rounded-control outline-none focus-visible:ring-2 focus-visible:ring-ring",
            className
          )}
        >
          {children}
        </button>
      </HoverCardTrigger>
      <span id={id} className="sr-only">
        {why} {changes.join(". ")}.
      </span>
      <HoverCardContent side={side} className="flex flex-col gap-2">
        <p>{why}</p>
        <ul className="m-0 flex list-none flex-col gap-0.5 p-0 text-muted-foreground">
          {changes.map((c) => (
            <li key={c}>{c}</li>
          ))}
        </ul>
        <button
          type="button"
          onClick={onOpenSettings}
          className="self-start text-[12px] text-foreground underline underline-offset-2 hover:text-fg-subtle"
        >
          Performance settings
        </button>
      </HoverCardContent>
    </HoverCard>
  );
}
