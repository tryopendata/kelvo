import { useLayoutEffect, useRef, useState } from "react";

/**
 * One content-box dimension of the element on `ref`, tracked with a
 * ResizeObserver as the window resizes. Starts at `initial` until the first
 * measurement. A zero reading (the element hidden or detached) keeps the
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
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(([entry]) => {
      const next = entry?.contentRect[axis] ?? 0;
      if (next > 0) setSize(next);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [axis]);
  return [ref, size] as const;
}
