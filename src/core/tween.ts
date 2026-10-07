/**
 * Retargetable numeric rAF tween, ported from opendata's
 * `shared/lib/tween.ts` (`createTween`). Callers mutate the DOM in `onFrame`
 * so a running tween never goes through a React render. No `window` access at
 * module scope.
 */
import { clamp01 } from "@core/chart-math";

export const lerp = (a: number, b: number, t: number): number =>
  a + (b - a) * t;

export type EasingFn = (t: number) => number;

/** Fast start, settles without overshoot. Matches `--ease-tick`. */
export const easeOutCubic: EasingFn = (t) => 1 - (1 - t) ** 3;

export interface TweenConfig {
  initial: number;
  ease?: EasingFn;
  onFrame: (value: number) => void;
  /** Called once when a tween reaches its target (not on a snap or cancel). */
  onDone?: () => void;
}

export interface Tween {
  /**
   * Retarget. A tween already running restarts from its live value, not from
   * the previous target, so a fast retarget never jumps. A duration of 0 (or
   * `snap`) applies the target synchronously.
   */
  to: (target: number, opts: { duration: number; snap?: boolean }) => void;
  get: () => number;
  running: () => boolean;
  cancel: () => void;
}

export function createTween(config: TweenConfig): Tween {
  const { ease = easeOutCubic, onFrame, onDone } = config;
  let current = config.initial;
  let rafId = 0;
  let active = false;

  const stop = () => {
    if (active) {
      cancelAnimationFrame(rafId);
      active = false;
    }
  };

  const to: Tween["to"] = (target, { duration, snap }) => {
    stop();
    if (snap || duration <= 0) {
      current = target;
      onFrame(current);
      return;
    }
    const from = current;
    const start = performance.now();
    active = true;
    const tick = (now: number) => {
      const t = clamp01((now - start) / duration);
      current = lerp(from, target, ease(t));
      onFrame(current);
      if (t < 1) {
        rafId = requestAnimationFrame(tick);
      } else {
        active = false;
        onDone?.();
      }
    };
    rafId = requestAnimationFrame(tick);
  };

  return { to, get: () => current, running: () => active, cancel: stop };
}
