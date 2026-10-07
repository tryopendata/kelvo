import { BRUSH_STEP_MS } from "@core/brush";
import { keepPreviousData } from "@tanstack/react-query";

interface QueryLike<T> {
  state: { data: T | undefined; status: string };
}

/**
 * Freshness for a query over a time range (D-089, D-099): an answer that
 * `isFinal` is read once; one that reaches time still being measured reads
 * again every 10 s until it is final; a failed one waits for the key to
 * move. `keepPrevious` holds the last answer while the key moves (the
 * whole-window view, whose key advances every 10 s).
 */
export function rangeQueryOptions<T>(
  isFinal: (data: T | undefined) => boolean,
  keepPrevious: boolean
) {
  return {
    staleTime: (q: QueryLike<T>) =>
      isFinal(q.state.data) ? Number.POSITIVE_INFINITY : 0,
    refetchInterval: (q: QueryLike<T>) =>
      q.state.status === "error" || isFinal(q.state.data)
        ? false
        : BRUSH_STEP_MS,
    placeholderData: keepPrevious
      ? (keepPreviousData as <D>(previous: D | undefined) => D | undefined)
      : undefined,
  };
}
