import type { TimeRange } from "@core/brush";
import {
  type KeyboardEvent,
  type PointerEvent,
  useEffect,
  useRef,
  useState,
} from "react";
import { selectedRange } from "../_lib/selection";

interface SelectionOptions {
  rangeKey: string;
  fromMs: number;
  toMs: number;
  bucketMs: number;
  width: number;
  nowMs: number;
  retainedFromMs: number;
  loading: boolean;
  onSelectRange?: ((range: TimeRange) => void) | undefined;
}
type Geometry = {
  from: number;
  to: number;
  bucket: number;
  left: number;
  width: number;
};

export function useTimelineSelection({
  fromMs,
  toMs,
  rangeKey,
  bucketMs,
  width,
  nowMs,
  retainedFromMs,
  loading,
  onSelectRange,
}: SelectionOptions) {
  const sliderRef = useRef<HTMLDivElement>(null);
  const [cursor, setCursor] = useState<number | null>(null);
  const [selection, setSelection] = useState<Selection>(null);
  const drag = useRef<Drag | null>(null);
  const geometry = useRef<Geometry | null>(null);
  const freeze = () => {
    geometry.current ??= plotGeometry(
      sliderRef.current,
      fromMs,
      toMs,
      bucketMs,
      width
    );
  };
  const focusAfterRange = useRef(false);
  const lastBucket = Math.max(fromMs, toMs - bucketMs);
  const moveCursor = (t: number) =>
    setCursor(clampCursor(t, geometry.current, fromMs, lastBucket));
  const cancelSelection = () => {
    geometry.current = null;
    drag.current = null;
    setSelection(null);
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: committed range identity resets; Live tail bounds do not
  useEffect(() => {
    cancelSelection();
    setCursor(null);
    if (focusAfterRange.current) {
      focusAfterRange.current = false;
      sliderRef.current?.focus({ preventScroll: true });
    }
  }, [rangeKey]);
  const snap = (a: number, b: number) =>
    selectedRange(
      a,
      b,
      geometry.current?.bucket ?? bucketMs,
      nowMs,
      retainedFromMs
    );
  const commitSelection = (a: number, b: number) => {
    const next = snap(a, b);
    cancelSelection();
    if (next) onSelectRange?.(next);
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const current = pointerTime(e, fromMs, toMs, geometry.current);
    if (current === null) return;
    if (drag.current) {
      if (Math.abs(e.clientX - drag.current.x) >= 4)
        setSelection({ anchor: drag.current.anchor, current });
    } else moveCursor(Math.floor(current / bucketMs) * bucketMs);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) =>
    selectionKeyboard(e, {
      loading,
      selection,
      cursor,
      canSelect: Boolean(onSelectRange),
      bounds: geometry.current ?? { from: fromMs, to: toMs, bucket: bucketMs },
      freeze,
      cancel: cancelSelection,
      commit: commitSelection,
      setSelection,
      moveCursor,
      unfreeze: () => {
        geometry.current = null;
      },
    });
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
        freeze();
        const anchor = pointerTime(e, fromMs, toMs, geometry.current);
        if (anchor === null) return;
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { pointer: e.pointerId, x: e.clientX, anchor };
        setSelection(null);
      },
      onPointerUp: (e: PointerEvent<HTMLDivElement>) => {
        const active = drag.current;
        if (!active || active.pointer !== e.pointerId) return;
        const current = pointerTime(e, fromMs, toMs, geometry.current);
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
  toMs: number,
  frozen: { from: number; to: number; left: number; width: number } | null
) {
  const rect = frozen ?? e.currentTarget.getBoundingClientRect();
  const from = frozen?.from ?? fromMs;
  const to = frozen?.to ?? toMs;
  return rect.width > 0
    ? from +
        Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width)) *
          (to - from)
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

function clampCursor(
  t: number,
  geometry: Geometry | null,
  from: number,
  last: number
) {
  return Math.min(
    geometry ? Math.max(geometry.from, geometry.to - geometry.bucket) : last,
    Math.max(geometry?.from ?? from, t)
  );
}
function keyMove(
  key: string,
  cursor: number | null,
  bounds: Pick<Geometry, "from" | "to" | "bucket">
) {
  const { from, to, bucket } = bounds;
  const last = Math.max(from, to - bucket);
  const at = cursor ?? last;
  const next = keyTime(key, at, from, last, bucket);
  return next === undefined
    ? null
    : {
        at,
        current: Math.min(
          last,
          Math.max(from, Math.floor(next / bucket) * bucket)
        ),
      };
}

type Selection = { anchor: number; current: number } | null;
function selectionKeyboard(
  e: KeyboardEvent<HTMLDivElement>,
  controls: {
    loading: boolean;
    selection: Selection;
    cursor: number | null;
    canSelect: boolean;
    bounds: Pick<Geometry, "from" | "to" | "bucket">;
    freeze: () => void;
    cancel: () => void;
    commit: (a: number, b: number) => void;
    setSelection: (selection: Selection) => void;
    moveCursor: (at: number) => void;
    unfreeze: () => void;
  }
) {
  if (e.key === "Escape") {
    e.preventDefault();
    controls.cancel();
    return;
  }
  if (e.key === "Enter" && controls.selection) {
    e.preventDefault();
    controls.commit(controls.selection.anchor, controls.selection.current);
    return;
  }
  if (controls.loading && !controls.selection) return;
  const extend = e.shiftKey && controls.canSelect;
  if (extend) controls.freeze();
  const move = keyMove(e.key, controls.cursor, controls.bounds);
  if (!move) return;
  e.preventDefault();
  if (extend)
    controls.setSelection({
      anchor: controls.selection?.anchor ?? move.at,
      current: move.current,
    });
  else {
    controls.unfreeze();
    controls.setSelection(null);
  }
  controls.moveCursor(move.current);
}

function plotGeometry(
  element: HTMLElement | null,
  from: number,
  to: number,
  bucket: number,
  width: number
): Geometry {
  const rect = element?.getBoundingClientRect();
  return {
    from,
    to,
    bucket,
    left: rect?.left ?? 0,
    width: rect?.width ?? width,
  };
}

type Drag = { pointer: number; x: number; anchor: number };
