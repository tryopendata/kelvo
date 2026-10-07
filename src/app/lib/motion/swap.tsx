import type { ReactNode } from "react";
import { cn } from "~/lib/utils";
import styles from "./motion.module.css";

export interface SwapProps {
  /** State the content shows; a new value remounts it and plays the fade. */
  k: string | number;
  className?: string;
  children: ReactNode;
}

/**
 * Keyed crossfade for a state change (paused to live, pressure turning
 * critical): the content remounts when `k` changes and fades in over
 * `--motion-crossfade`. Mounting with a new `k` plays it too.
 *
 * Leaf content only. Remounting destroys whatever is inside, so never wrap a
 * canvas, a uPlot chart, a live store subscriber or a `role="status"` region.
 */
export function Swap({ k, className, children }: SwapProps) {
  return (
    <span key={k} className={cn(styles.swap, className)}>
      {children}
    </span>
  );
}
