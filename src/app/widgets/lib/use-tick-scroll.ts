import { useLayoutEffect, useRef } from "react";

/**
 * Streaming scroll (design-system.md "Motion"): when the window advances by
 * exactly one interval, the already-redrawn content slides from where the old
 * content was (`translateX(shift)`) to rest over `--motion-tick`. The
 * path `d` is never tweened. Any other jump (first render, a gap, an interval
 * change) is drawn in place. Read the root motion tokens each tick so reduced
 * motion and Performance mode land immediately. Explicit animation keyframes
 * avoid forcing geometry to commit a CSS transition's starting offset.
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
    const style = getComputedStyle(el.ownerDocument.documentElement);
    const duration = Number.parseFloat(style.getPropertyValue("--motion-tick"));
    if (!(duration > 0) || typeof el.animate !== "function") return;
    const animation = el.animate(
      [{ transform: `translateX(${shift})` }, { transform: "translateX(0)" }],
      { duration, easing: style.getPropertyValue("--ease-tick").trim() }
    );
    return () => animation.cancel();
  }, [tEndMs, intervalMs, shift]);

  return ref;
}
