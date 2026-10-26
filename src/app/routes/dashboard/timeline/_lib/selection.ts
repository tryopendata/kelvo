import type { TimeRange } from "@core/brush";

/** Selection follows displayed bucket edges, never inventing finer precision. */
export function selectedRange(
  a: number,
  b: number,
  bucketMs: number,
  nowMs: number,
  retainedFrom = 0
): TimeRange | null {
  const minimum = Math.max(60_000, bucketMs);
  let fromMs = Math.floor(Math.min(a, b) / bucketMs) * bucketMs;
  let toMs = Math.ceil(Math.max(a, b) / bucketMs) * bucketMs;
  if (toMs - fromMs < minimum) toMs = fromMs + minimum;
  toMs = Math.min(toMs, nowMs);
  fromMs = Math.max(retainedFrom, Math.min(fromMs, toMs - minimum));
  return toMs - fromMs >= minimum ? { fromMs, toMs } : null;
}
