import type { TimeRange } from "@core/brush";
import {
  type RefObject,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";

interface Preview {
  target: TimeRange;
  source: TimeRange;
  resolution: string;
  sawLoading: boolean;
}

/** A one-shot raster/DOM projection of existing plots, never a second chart subscriber. */
export function useZoomPreview(
  plotRef: RefObject<HTMLDivElement | null>,
  bounds: TimeRange,
  resolution: string,
  loading: boolean,
  hostId: string,
  failed = false
) {
  const overlayRef = useRef<HTMLDivElement>(null);
  const run = useRef<{ preview: Preview; animation: Animation | null } | null>(
    null
  );
  const [preview, setPreview] = useState<Preview | null>(null);
  const cancel = useCallback(() => {
    run.current?.animation?.cancel();
    run.current = null;
    overlayRef.current?.replaceChildren();
    setPreview(null);
  }, []);
  const begin = (target: TimeRange) => {
    cancel();
    if (failed) return;
    const source = plotRef.current?.querySelector<HTMLElement>(
      "[data-lane-content]"
    );
    const overlay = overlayRef.current;
    if (!source || !overlay || document.hidden) return;
    const style = getComputedStyle(document.documentElement);
    const duration =
      Number.parseFloat(style.getPropertyValue("--motion-enter")) || 0;
    if (duration <= 0) return;
    const clone = clonePlot(source);
    const width = source.getBoundingClientRect().width;
    const targetDuration = target.toMs - target.fromMs;
    const scale = (bounds.toMs - bounds.fromMs) / targetDuration;
    const translate =
      ((bounds.fromMs - target.fromMs) / targetDuration) * width;
    const transform = `translateX(${translate}px) scaleX(${scale})`;
    clone.style.transform = transform;
    overlay.style.opacity = "1";
    overlay.replaceChildren(clone);
    const next: Preview = {
      target,
      source: bounds,
      resolution,
      sawLoading: false,
    };
    const animation = clone.animate(
      [{ transform: "translateX(0) scaleX(1)" }, { transform }],
      {
        duration,
        easing: style.getPropertyValue("--ease-out").trim() || "ease-out",
      }
    );
    run.current = { preview: next, animation };
    setPreview(next);
  };
  useEffect(() => {
    const current = run.current;
    if (!current) return;
    const matches = sameRange(
      current.preview.target,
      bounds.fromMs,
      bounds.toMs
    );
    const original = sameRange(
      current.preview.source,
      bounds.fromMs,
      bounds.toMs
    );
    if (!matches && !original) {
      cancel();
      return;
    }
    if (failed) {
      cancel();
      return;
    }
    if (loading) {
      current.preview.sawLoading = true;
      return;
    }
    if (!matches) {
      if (current.preview.sawLoading) cancel();
      return;
    }
    const finish = async () => {
      await current.animation?.finished.catch(() => undefined);
      if (run.current !== current) return;
      const element = overlayRef.current;
      if (!element) {
        cancel();
        return;
      }
      const duration =
        Number.parseFloat(
          getComputedStyle(document.documentElement).getPropertyValue(
            "--motion-crossfade"
          )
        ) || 0;
      const fade = element.animate([{ opacity: 1 }, { opacity: 0 }], {
        duration,
      });
      current.animation = fade;
      await fade.finished.catch(() => undefined);
      if (run.current === current) cancel();
    };
    void finish();
  }, [loading, failed, bounds.fromMs, bounds.toMs, cancel]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: host is a cancellation boundary
  useEffect(() => cancel(), [hostId, cancel]);
  useEffect(() => {
    const stop = () => {
      if (document.hidden) cancel();
    };
    document.addEventListener("visibilitychange", stop);
    return () => {
      document.removeEventListener("visibilitychange", stop);
      run.current?.animation?.cancel();
    };
  }, [cancel]);
  return { overlayRef, preview, begin, cancel };
}

const sameRange = (a: TimeRange, fromMs: number, toMs: number) =>
  a.fromMs === fromMs && a.toMs === toMs;

/** cloneNode does not copy a canvas backing store. Copy its already-painted pixels. */
function clonePlot(source: HTMLElement): HTMLElement {
  const clone = source.cloneNode(true) as HTMLElement;
  const copies = clone.querySelectorAll("canvas");
  source.querySelectorAll("canvas").forEach((canvas, index) => {
    const copy = copies[index];
    if (!copy) return;
    copy.width = canvas.width;
    copy.height = canvas.height;
    copy.getContext("2d")?.drawImage(canvas, 0, 0);
  });
  clone.inert = true;
  clone.setAttribute("aria-hidden", "true");
  clone.style.transformOrigin = "left center";
  return clone;
}
