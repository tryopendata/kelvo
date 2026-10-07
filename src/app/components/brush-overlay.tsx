import {
  BRUSH_STEP_MS,
  bucketAt,
  clampRange,
  extendRange,
  floorTo,
  rangeFractions,
  snapRange,
  type TimeRange,
  timeAt,
} from "@core/brush";
import { formatClockSeconds } from "@core/format";
import {
  type KeyboardEvent,
  type PointerEvent,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useBrush, useBrushStore } from "~/stores/brush-store";

/** Pointer travel, px, below which a press is a click rather than a drag. */
const CLICK_SLOP_PX = 3;

/**
 * The chart and the views that read its selection mark themselves with this
 * attribute; a press inside one never clears the selection from outside.
 */
export const BRUSH_SCOPE_ATTR = "data-brush-scope";

/** Presses on these act on their own, so they never clear the selection. */
const INTERACTIVE = [
  "button, a, input, select, textarea, label, summary, [contenteditable=true]",
  "[role=button], [role=link], [role=tab], [role=radio], [role=checkbox], [role=switch], [role=slider]",
  "[role=menuitem], [role=menuitemradio], [role=menuitemcheckbox], [role=option], [role=listbox], [role=dialog]",
].join(", ");

/**
 * Clears the selection the conventional ways while there is one: Esc
 * anywhere on the page (not while typing in a field or with a dialog open),
 * and a press on empty space outside the chart and its views.
 */
function useDismissOutside(clear: () => void, active: boolean) {
  useEffect(() => {
    if (!active) return;
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      const t = e.target instanceof Element ? e.target : null;
      if (t?.closest("input, textarea, select, [contenteditable=true]")) return;
      if (document.querySelector("[role=dialog], [role=alertdialog]")) return;
      clear();
    };
    const onDown = (e: globalThis.PointerEvent) => {
      if (e.button !== 0) return;
      const t = e.target instanceof Element ? e.target : null;
      if (!t || t.closest(`[${BRUSH_SCOPE_ATTR}]`) || t.closest(INTERACTIVE)) {
        return;
      }
      clear();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("pointerdown", onDown);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("pointerdown", onDown);
    };
  }, [clear, active]);
}

export interface BrushOverlayProps {
  /** Start of the chart's first bar. */
  firstMs: number;
  /** Time the chart spans, first bar's start to last bar's end. */
  spanMs: number;
  /** Height of the bars, px. */
  height: number;
  /**
   * End of the selectable time: the newest elapsed 10 s edge. The last bar
   * reaches into time that has not happened yet; that part cannot be
   * selected or focused. Defaults to the chart's end.
   */
  selectableToMs?: number;
}

const pct = (f: number) => `${(f * 100).toFixed(3)}%`;

/**
 * The selection layer over a brushable live chart (D-089): a
 * crosshair and a 10 s hover column, drag to select, click for the 10 s
 * bucket under the pointer. A selection clears like a d3 brush's: a plain
 * click on the chart, Esc anywhere on the page, or a press on empty space
 * outside the chart and its views (D-093). Focusable: arrows move a 10 s
 * focus, Enter selects it, Shift+arrows extend the selection.
 *
 * The hover column is moved through its ref, never through state, so a
 * pointer passing over the chart re-renders nothing; only a drag and a
 * change of selection do.
 *
 * Exposed as a slider over the selectable 10 s buckets: its value is the
 * focused bucket's position, its text that bucket's times and then the
 * selection, and its description the keys.
 */
export function BrushOverlay({
  firstMs,
  spanMs,
  height,
  selectableToMs,
}: BrushOverlayProps) {
  const store = useBrushStore();
  const view = useBrush((s) => s.draft ?? s.range);
  const range = useBrush((s) => s.range);
  const hoverRef = useRef<HTMLDivElement>(null);
  const hintId = useId();
  const toMs = Math.min(firstMs + spanMs, selectableToMs ?? firstMs + spanMs);
  const geo = useRef({ firstMs, spanMs, toMs });
  useLayoutEffect(() => {
    geo.current = { firstMs, spanMs, toMs };
  }, [firstMs, spanMs, toMs]);
  const drag = useRef<{ anchorMs: number; x: number; moved: boolean } | null>(
    null
  );
  const anchor = useRef<number | null>(null);
  const [focusMs, setFocusMs] = useState<number | null>(null);
  useDismissOutside(store.getState().clear, range !== null);

  const bounds = () => {
    const g = geo.current;
    return { lo: g.firstMs, hi: g.toMs };
  };
  const clamp = (r: TimeRange) => {
    const { lo, hi } = bounds();
    return clampRange(r, lo, hi);
  };
  const timeOf = (e: PointerEvent<HTMLDivElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    const f = box.width > 0 ? (e.clientX - box.left) / box.width : 0;
    const g = geo.current;
    // The right edge belongs to the last selectable bucket, not the time
    // after it.
    return Math.min(timeAt(f, g.firstMs, g.spanMs), g.toMs - 1);
  };

  const hideHover = () => {
    if (hoverRef.current) hoverRef.current.hidden = true;
  };
  const showHover = (tMs: number) => {
    const el = hoverRef.current;
    if (!el) return;
    const g = geo.current;
    const f = rangeFractions(bucketAt(tMs), g.firstMs, g.spanMs);
    if (!f) {
      el.hidden = true;
      return;
    }
    el.style.left = pct(f.left);
    el.style.width = pct(f.width);
    el.hidden = false;
  };

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    drag.current = { anchorMs: timeOf(e), x: e.clientX, moved: false };
  };
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    const t = timeOf(e);
    showHover(t);
    const d = drag.current;
    if (!d) return;
    if (!d.moved && Math.abs(e.clientX - d.x) < CLICK_SLOP_PX) return;
    d.moved = true;
    store.getState().setDraft(clamp(snapRange(d.anchorMs, t)));
  };
  const onPointerUp = (e: PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    const s = store.getState();
    if (!d.moved && s.range !== null) {
      // A click with a selection up dismisses it; a drag replaces it.
      s.clear();
      anchor.current = null;
      return;
    }
    const picked = d.moved
      ? clamp(snapRange(d.anchorMs, timeOf(e)))
      : clamp(bucketAt(d.anchorMs));
    s.select(picked);
    anchor.current = picked.fromMs;
  };
  const onPointerCancel = () => {
    drag.current = null;
    store.getState().setDraft(null);
  };

  const focusBounds = () => {
    const { lo, hi } = bounds();
    return { first: floorTo(lo), last: floorTo(hi - 1) };
  };
  /** Where focus lands: the selection's last bucket, else the newest one. */
  const startFocus = (r: TimeRange | null) => {
    const { first, last } = focusBounds();
    const start = r ? r.toMs - BRUSH_STEP_MS : last;
    return Math.min(last, Math.max(first, start));
  };
  const onFocus = () => {
    if (focusMs !== null) return;
    setFocusMs(startFocus(store.getState().range));
  };
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const s = store.getState();
    if (e.key === "Escape") {
      if (s.range === null && s.draft === null) return;
      e.preventDefault();
      s.clear();
      anchor.current = null;
      return;
    }
    const { first, last } = focusBounds();
    const cur = Math.min(last, Math.max(first, focusMs ?? last));
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      s.select(bucketAt(cur));
      anchor.current = cur;
      return;
    }
    const step = e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : null;
    if (step === null) return;
    e.preventDefault();
    const next = Math.min(last, Math.max(first, cur + step * BRUSH_STEP_MS));
    setFocusMs(next);
    if (!e.shiftKey) {
      anchor.current = null;
      return;
    }
    if (anchor.current === null) {
      // Extend from the end of the selection away from the focus, or start
      // one at the bucket the focus was on.
      const r = s.range;
      anchor.current =
        r && cur >= r.fromMs && cur < r.toMs
          ? step > 0
            ? r.fromMs
            : r.toMs - BRUSH_STEP_MS
          : cur;
    }
    s.select(extendRange(anchor.current, next));
  };

  const band = view ? rangeFractions(view, firstMs, spanMs) : null;
  const focus =
    focusMs === null
      ? null
      : rangeFractions(bucketAt(focusMs), firstMs, spanMs);
  // The bucket the slider is on: focused, or where focus will land.
  const first = floorTo(firstMs);
  const last = floorTo(toMs - 1);
  const at = Math.min(
    last,
    Math.max(first, focusMs ?? (range ? range.toMs - BRUSH_STEP_MS : last))
  );
  const span = (r: TimeRange) =>
    `${formatClockSeconds(r.fromMs)} to ${formatClockSeconds(r.toMs)}`;
  const valueText = `${span(bucketAt(at))}, ${
    view ? `selected ${span(view)}` : "no selection"
  }`;

  return (
    <div
      role="slider"
      tabIndex={0}
      aria-label="Select a time range"
      aria-describedby={hintId}
      aria-valuemin={0}
      aria-valuemax={Math.max(0, (last - first) / BRUSH_STEP_MS)}
      aria-valuenow={(at - first) / BRUSH_STEP_MS}
      aria-valuetext={valueText}
      data-testid="brush"
      className="group absolute top-0 right-0 left-24 cursor-crosshair touch-none select-none rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
      style={{ height }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerCancel}
      onPointerLeave={hideHover}
      onFocus={onFocus}
      onBlur={() => setFocusMs(null)}
      onKeyDown={onKeyDown}
    >
      <span id={hintId} className="sr-only">
        Arrow keys move, Enter selects, Shift+arrows extend, Escape clears
      </span>
      <div
        ref={hoverRef}
        hidden
        data-testid="brush-hover"
        className="pointer-events-none absolute inset-y-0 bg-foreground/4"
      />
      {band && (
        <div
          data-testid="brush-band"
          className="pointer-events-none absolute inset-y-0 border-foreground/35 border-x bg-foreground/5"
          style={{ left: pct(band.left), width: pct(band.width) }}
        />
      )}
      {focus && (
        <div
          data-testid="brush-focus"
          className="pointer-events-none absolute inset-y-0 hidden bg-foreground/6 outline outline-1 outline-ring group-focus-visible:block"
          style={{ left: pct(focus.left), width: pct(focus.width) }}
        />
      )}
    </div>
  );
}
