import { useEffect, useRef, useState } from "react";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "~/components/ui/tooltip";

/** How long "Copied" stays up after a click. */
const COPIED_MS = 1500;

/**
 * A value that copies itself on click: the tooltip says "Copy" on hover and
 * "Copied" once the clipboard has it ("Couldn't copy" if it refused).
 */
export function CopyValue({ value, label }: { value: string; label: string }) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const [hover, setHover] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    []
  );

  const copy = async () => {
    let next: "copied" | "failed";
    try {
      await navigator.clipboard.writeText(value);
      next = "copied";
    } catch (err) {
      console.error("[copy-value] copy failed", { label, err });
      next = "failed";
    }
    setState(next);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setState("idle"), COPIED_MS);
  };

  return (
    <Tooltip
      open={state !== "idle" || hover}
      onOpenChange={(open) => setHover(open)}
    >
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={() => void copy()}
          aria-label={`Copy ${label} ${value}`}
          className="figures -mx-1 rounded-sm px-1 text-fg-subtle outline-none hover:bg-selected hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          {value}
        </button>
      </TooltipTrigger>
      <TooltipContent>
        <span aria-live="polite">
          {state === "copied"
            ? "Copied"
            : state === "failed"
              ? "Couldn't copy"
              : "Copy"}
        </span>
      </TooltipContent>
    </Tooltip>
  );
}
