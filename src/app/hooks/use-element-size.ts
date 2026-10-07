import { useLayoutEffect, useRef, useState } from "react";

/**
 * One content-box dimension of the element on `ref`, tracked with a
 * ResizeObserver as the window resizes. Measured before the first paint;
 * `initial` stands in until a non-zero reading. A zero reading (the element hidden or detached) keeps the
 * last size. Only the named axis is state, so a change on the other axis
 * re-renders nothing.
 */
export function useElementSize<T extends Element = HTMLDivElement>(
  axis: "width" | "height",
  initial = 0
) {
  const ref = useRef<T>(null);
  const [size, setSize] = useState(initial);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    // Measure once before paint so the first frame isn't drawn at `initial`.
    // The border box less padding and border is the content box the
    // observer reports.
    const box = el.getBoundingClientRect()[axis];
    if (box > 0) {
      const s = getComputedStyle(el);
      const sides = axis === "width" ? ["left", "right"] : ["top", "bottom"];
      const inset = sides.reduce(
        (sum, side) =>
          sum +
          (Number.parseFloat(s.getPropertyValue(`padding-${side}`)) || 0) +
          (Number.parseFloat(s.getPropertyValue(`border-${side}-width`)) || 0),
        0
      );
      if (box - inset > 0) setSize(box - inset);
    }
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(([entry]) => {
      const next = entry?.contentRect[axis] ?? 0;
      if (next > 0) setSize(next);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [axis]);
  return [ref, size] as const;
}
