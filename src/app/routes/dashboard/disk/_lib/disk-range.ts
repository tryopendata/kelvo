import type { TimeRange } from "@core/brush";

export function diskRange(params: URLSearchParams): TimeRange | null {
  const fromMs = Number(params.get("from"));
  const toMs = Number(params.get("to"));
  return params.has("from") &&
    Number.isSafeInteger(fromMs) &&
    Number.isSafeInteger(toMs) &&
    fromMs >= 0 &&
    toMs > fromMs
    ? { fromMs, toMs }
    : null;
}
