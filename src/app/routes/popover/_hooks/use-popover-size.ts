import { useLayoutEffect, useRef } from "react";
import { useTransport } from "~/lib/transport-context";

/** Observe intrinsic regions, never the viewport; unchanged metric text cannot resize it. */
export function usePopoverSize(ready: boolean, layoutKey: string) {
  const root = useRef<HTMLDivElement>(null);
  const transport = useTransport();
  const lastReported = useRef(0);
  useLayoutEffect(() => {
    if (
      !ready ||
      layoutKey.length === 0 ||
      !root.current ||
      typeof ResizeObserver === "undefined"
    )
      return;
    const regions = Array.from(
      root.current.querySelectorAll<HTMLElement>("[data-popover-size]")
    );
    let frame = 0;
    const report = () => {
      frame = 0;
      const height = Math.ceil(
        regions.reduce(
          (total, region) => total + region.getBoundingClientRect().height,
          0
        )
      );
      if (height > 0 && height !== lastReported.current) {
        lastReported.current = height;
        void transport
          .reportPopoverContentHeight(height)
          .catch((error: unknown) =>
            console.error("[popover] content size failed", error)
          );
      }
    };
    const observer = new ResizeObserver(() => {
      if (!frame) frame = requestAnimationFrame(report);
    });
    for (const region of regions) observer.observe(region);
    report();
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [ready, transport, layoutKey]);
  return root;
}
