import type { CSSProperties } from "react";
import styles from "./motion.module.css";

export type EnterVariant = "lift" | "fade";

/**
 * Entrance props for the item at `index` in a surface's reading order: a fade
 * plus a 3 px rise (`lift`) or a plain fade, delayed by `index` stagger steps
 * (capped). Spread onto an existing element so grids and stacks gain no
 * wrapper:
 *
 *   const e = enter(i);
 *   <div className={cn("...", e.className)} style={{ ...style, ...e.style }} />
 *
 * Runs once per mount. A dashboard route change mounts a fresh page, so pages
 * replay on navigation with no trigger.
 */
export function enter(
  index: number,
  variant: EnterVariant = "lift"
): { className: string; style: CSSProperties } {
  return {
    className: styles[variant] ?? "",
    style: { "--i": index } as CSSProperties,
  };
}

/**
 * Class for the element that hosts a route's `<Outlet />`: each child of the
 * route's root lifts in reading order on navigation. A child that staggers
 * its own children (CardGrid) carries `data-stagger` and `stagger(i)` on them.
 */
export const pageEnter: string = styles.page ?? "";

/** Inline style for the `index`th child of a `data-stagger` container. */
export function stagger(index: number): CSSProperties {
  return { "--j": index } as CSSProperties;
}
