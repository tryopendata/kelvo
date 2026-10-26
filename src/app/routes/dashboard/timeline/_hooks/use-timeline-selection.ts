import type { TimeRange } from "@core/brush";
import {
  type KeyboardEvent,
  type PointerEvent,
  useEffect,
  useRef,
  useState,
} from "react";
import { selectedRange } from "../_lib/selection";

export function useTimelineSelection({
  fromMs,
  toMs,
  bucketMs,
  width,
  nowMs,
  retainedFromMs,
  loading,
  onSelectRange,
}: {
  fromMs: number;
  toMs: number;
  bucketMs: number;
  width: number;
  nowMs: number;
  retainedFromMs: number;
  loading: boolean;
  onSelectRange?: ((range: TimeRange) => void) | undefined;
}) {
  const sliderRef = useRef<HTMLDivElement>(null);
  const [cursor, setCursor] = useState<number | null>(null);
  const [selection, setSelection] = useState<{
    anchor: number;
    current: number;
  } | null>(null);
  const drag = useRef<{ pointer: number; x: number; anchor: number } | null>(
    null
  );
  const keyboardAnchor = useRef<number | null>(null);
  const focusAfterRange = useRef(false);
  const lastBucket = Math.max(fromMs, toMs - bucketMs);
  const moveCursor = (t: number) =>
    setCursor(Math.min(lastBucket, Math.max(fromMs, t)));
  const cancelSelection = () => {
    drag.current = null;
    keyboardAnchor.current = null;
    setSelection(null);
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: bounds are the reset trigger
  useEffect(() => {
    cancelSelection();
    setCursor(null);
    if (focusAfterRange.current) {
      focusAfterRange.current = false;
      sliderRef.current?.focus({ preventScroll: true });
    }
  }, [fromMs, toMs]);
  const snap = (a: number, b: number) =>
    selectedRange(a, b, bucketMs, nowMs, retainedFromMs);
  const commitSelection = (a: number, b: number) => {
    const next = snap(a, b);
    cancelSelection();
    if (next) onSelectRange?.(next);
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const current = pointerTime(e, fromMs, toMs);
    if (current === null) return;
    if (drag.current) {
      if (Math.abs(e.clientX - drag.current.x) >= 4)
        setSelection({ anchor: drag.current.anchor, current });
    } else moveCursor(Math.floor(current / bucketMs) * bucketMs);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault();
      cancelSelection();
      return;
    }
    if (e.key === "Enter" && selection) {
      e.preventDefault();
      commitSelection(selection.anchor, selection.current);
      return;
    }
    if (loading) return;
    const at = cursor ?? lastBucket;
    const next = keyTime(e.key, at, fromMs, lastBucket, bucketMs);
    if (next === undefined) return;
    e.preventDefault();
    const current = Math.min(
      lastBucket,
      Math.max(fromMs, Math.floor(next / bucketMs) * bucketMs)
    );
    if (e.shiftKey && onSelectRange) {
      keyboardAnchor.current ??= at;
      setSelection({ anchor: keyboardAnchor.current, current });
    } else {
      keyboardAnchor.current = null;
      setSelection(null);
    }
    moveCursor(current);
  };
  return {
    sliderRef,
    cursor,
    selecting: selection !== null,
    focusAfterRange,
    preview: selection ? snap(selection.anchor, selection.current) : null,
    selectEvent: (tMs: number) => {
      moveCursor(Math.floor(tMs / bucketMs) * bucketMs);
      sliderRef.current?.focus();
    },
    sliderProps: {
      onPointerDown: (e: PointerEvent<HTMLDivElement>) => {
        if (!onSelectRange || width <= 0 || loading || e.button !== 0) return;
        const anchor = pointerTime(e, fromMs, toMs);
        if (anchor === null) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { pointer: e.pointerId, x: e.clientX, anchor };
        setSelection(null);
      },
      onPointerUp: (e: PointerEvent<HTMLDivElement>) => {
        const active = drag.current;
        if (!active || active.pointer !== e.pointerId) return;
        const current = pointerTime(e, fromMs, toMs);
        if (current !== null && Math.abs(e.clientX - active.x) >= 4)
          commitSelection(active.anchor, current);
        else {
          cancelSelection();
          if (current !== null)
            moveCursor(Math.floor(current / bucketMs) * bucketMs);
        }
        if (e.currentTarget.hasPointerCapture(e.pointerId))
          e.currentTarget.releasePointerCapture(e.pointerId);
      },
      onPointerCancel: cancelSelection,
      onLostPointerCapture: () => {
        if (drag.current) cancelSelection();
      },
      onPointerMove,
      onPointerLeave: () => {
        if (!drag.current) setCursor(null);
      },
      onKeyDown,
      onFocus: () => setCursor((c) => c ?? lastBucket),
      onBlur: () => {
        setCursor(null);
        cancelSelection();
      },
    },
  };
}

function pointerTime(
  e: PointerEvent<HTMLDivElement>,
  fromMs: number,
  toMs: number
) {
  const rect = e.currentTarget.getBoundingClientRect();
  return rect.width > 0
    ? fromMs +
        Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width)) *
          (toMs - fromMs)
    : null;
}

function keyTime(
  key: string,
  at: number,
  from: number,
  last: number,
  step: number
): number | undefined {
  const positions: Record<string, number> = {
    ArrowLeft: at - step,
    ArrowRight: at + step,
    Home: from,
    End: last,
  };
  return positions[key];
}
