import { type RefObject, useEffect, useRef } from "react";

/** Edge scrolling exists only during an active gutter drag; no idle frame loop. */
export function useProcessDrag(
  root: RefObject<HTMLDivElement | null>,
  dragging: RefObject<boolean>,
  over: (key: string) => void
) {
  const latest = useRef(over);
  latest.current = over;
  useEffect(() => {
    let frame = 0;
    let x = 0;
    let y = 0;
    const stop = () => {
      dragging.current = false;
      cancelAnimationFrame(frame);
      frame = 0;
    };
    // biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Existing cohesive logic; split when substantially changed (architecture refactor section 10).
    const tick = () => {
      frame = 0;
      const surface = root.current;
      if (!dragging.current || !surface) return;
      const scroller =
        surface.querySelector<HTMLElement>("[data-process-scroll]") ??
        surface.closest("main");
      if (scroller) {
        const rect = scroller.getBoundingClientRect();
        const top = Math.max(0, rect.top);
        const bottom = Math.min(window.innerHeight, rect.bottom);
        const delta = y < top + 32 ? -12 : y > bottom - 32 ? 12 : 0;
        if (delta) scroller.scrollTop += delta;
        const hit = document.elementFromPoint(
          x,
          Math.min(bottom - 2, Math.max(top + 2, y))
        );
        const row = hit?.closest<HTMLElement>("[data-process-key]");
        if (row?.dataset.processKey && surface.contains(row))
          latest.current(row.dataset.processKey);
      }
      frame = requestAnimationFrame(tick);
    };
    const move = (event: PointerEvent) => {
      if (!dragging.current) return;
      x = event.clientX;
      y = event.clientY;
      if (!frame) frame = requestAnimationFrame(tick);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
    window.addEventListener("pointercancel", stop);
    window.addEventListener("blur", stop);
    return () => {
      stop();
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      window.removeEventListener("pointercancel", stop);
      window.removeEventListener("blur", stop);
    };
  }, [root, dragging]);
}
