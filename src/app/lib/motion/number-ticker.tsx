import { type Figure, figureAt, parseFigure, tickPath } from "@core/format";
import { createTween, type Tween } from "@core/tween";
import { useEffect, useLayoutEffect, useRef } from "react";

/**
 * `--motion-count` in ms: 0 under reduced motion and Performance mode. Read
 * from the root, where the tokens and `data-performance` live.
 */
function countMs(el: Element): number {
  const v = getComputedStyle(el.ownerDocument.documentElement).getPropertyValue(
    "--motion-count"
  );
  const ms = Number.parseFloat(v);
  return Number.isFinite(ms) ? ms : 0;
}

/**
 * A formatted number that counts to its next value instead of jumping
 * (opendata's `RollingNumber`): "10 GB" to "100 MB" counts down through the
 * megabytes. Small moves (under 10%), a change of kind ("—", another unit)
 * or `unit` changing replace the text in place. The count writes the span's
 * text directly, so it never re-renders React, and its last frame is the
 * exact `text`. Over `--motion-count`; off under reduced motion and power
 * saver.
 *
 * `unit` is for figures whose unit sits outside the text (a ring's "GB USED"
 * label): when it changes the number can't be counted, so it lands.
 */
export function NumberTicker({ text, unit }: { text: string; unit?: string }) {
  const ref = useRef<HTMLSpanElement>(null);
  const run = useRef<{
    text: string;
    unit: string | undefined;
    /** What is on screen now, with its in-between value. */
    shown: Figure | null;
    target: Figure | null;
    log: boolean;
    tween: Tween | null;
  } | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const to = parseFigure(text);
    const r = run.current;
    if (!r) {
      run.current = {
        text,
        unit,
        shown: to,
        target: to,
        log: false,
        tween: null,
      };
      return;
    }
    if (r.text === text && r.unit === unit) return;
    const path = r.unit === unit ? tickPath(r.shown, to) : null;
    const duration = path ? countMs(el) : 0;
    r.text = text;
    r.unit = unit;
    if (!path || !to || duration <= 0) {
      // React has already written `text`; a running count must not overwrite it.
      r.tween?.cancel();
      r.shown = to;
      r.target = to;
      return;
    }

    r.target = to;
    if (!r.tween) {
      r.tween = createTween({
        initial: path.from,
        onFrame: (v) => {
          const target = run.current?.target;
          const node = ref.current;
          if (!target || !node || !run.current) return;
          const base = run.current.log ? Math.exp(v) : v;
          run.current.shown = { ...target, base };
          node.textContent = figureAt(target, base);
        },
        onDone: () => {
          const cur = run.current;
          if (!cur || !ref.current) return;
          cur.shown = cur.target;
          ref.current.textContent = cur.text;
        },
      });
    }
    // A count already running in the same space carries on from where it is;
    // otherwise start from the old value, overwriting the text React wrote.
    if (!r.tween.running() || r.log !== path.log) {
      r.log = path.log;
      r.tween.to(path.from, { duration: 0 });
    }
    r.tween.to(path.to, { duration });
  }, [text, unit]);

  useEffect(() => () => run.current?.tween?.cancel(), []);

  return <span ref={ref}>{text}</span>;
}
