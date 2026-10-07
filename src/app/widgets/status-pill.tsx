import { Pause } from "lucide-react";
import { Swap } from "~/lib/motion/swap";
import { cn } from "~/lib/utils";

export interface StatusPillProps {
  state: "live" | "paused" | "stale";
  /** "Live", "1s", "Paused", "Stale". */
  label: string;
}

/**
 * Dot plus short label. Live is a flat 6 px dot in
 * `--color-live` that never pulses; paused carries a pause glyph; stale a
 * hollow dot. The word is always present, so the state never rests on color.
 * A state change crossfades the glyph and word; a new label alone does not.
 */
export function StatusPill({ state, label }: StatusPillProps) {
  return (
    <span
      data-state={state}
      className={cn(
        "inline-flex h-6 items-center gap-1.5 rounded-full border border-border bg-btn px-2.5 text-[11px]",
        state === "live" ? "text-fg-subtle" : "text-muted-foreground"
      )}
    >
      <Swap k={state} className="inline-flex items-center gap-1.5">
        {state === "paused" ? (
          <Pause aria-hidden className="size-2.5" strokeWidth={2.5} />
        ) : (
          <span
            aria-hidden
            className={cn(
              "size-1.5 rounded-full",
              state === "live"
                ? "bg-live"
                : "border border-muted-foreground bg-transparent"
            )}
          />
        )}
        <span className="data-mono">{label}</span>
      </Swap>
    </span>
  );
}
