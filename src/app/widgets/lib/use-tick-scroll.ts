import { useLayoutEffect, useRef } from "react";

/**
 * Streaming scroll (design-system.md "Motion"): when the window advances by
 * exactly one interval, the already-redrawn content is put back where the old
 * content was (`translateX(shift)`) and slid to rest over `--motion-tick`. The
 * path `d` is never tweened. Any other jump (first render, a gap, an interval
 * change) is drawn in place. `--motion-tick` is 0ms under reduced motion and
 * Performance mode, so the slide becomes a jump there without a JS branch.
 *
 * `shift` is a CSS length: user units for an SVG `<g>` (`"16.9px"`), or a
 * percentage of the element's own width for an HTML row (`"2.08%"`).
 */
export function useTickScroll<T extends HTMLElement | SVGGElement>(
  tEndMs: number | undefined,
  intervalMs: number,
  shift: string
) {
  const ref = useRef<T>(null);
  const prev = useRef<number | undefined>(undefined);

  useLayoutEffect(() => {
    const el = ref.current;
    const last = prev.current;
    prev.current = tEndMs;
    if (!el || tEndMs === undefined || last === undefined) return;
    const step = tEndMs - last;
    if (Math.abs(step - intervalMs) > intervalMs / 2) return;
    el.style.transition = "none";
    el.style.transform = `translateX(${shift})`;
    // Commit the offset before the transition starts.
    el.getBoundingClientRect();
    el.style.transition = "transform var(--motion-tick) var(--ease-tick)";
    el.style.transform = "translateX(0)";
  }, [tEndMs, intervalMs, shift]);

  return ref;
}
