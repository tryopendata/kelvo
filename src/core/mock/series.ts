import {
  HOLD_FACTOR,
  type SeriesKey,
  type SeriesSelector,
} from "@core/generated/bindings";

/** Whether `key` has `sel`'s metric and every one of its labels. */
export function matches(key: SeriesKey, sel: SeriesSelector): boolean {
  return (
    key.metric === sel.metric &&
    sel.labels.every(([k, v]) =>
      key.labels.some(([kk, vv]) => kk === k && vv === v)
    )
  );
}

/**
 * How long a sample taken every `periodMs` stays current: `HOLD_FACTOR`
 * times its period in integer milliseconds, the engine's `hold_ms`
 * (`kelvo-engine/src/engine.rs`, D-047, D-090).
 */
export function holdMs(periodMs: number): number {
  return Math.floor((periodMs * HOLD_FACTOR.num) / HOLD_FACTOR.den);
}
