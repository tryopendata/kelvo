import { useEffect, useState } from "react";

/**
 * A value read from the wall clock that only changes at a known moment, such
 * as the current local hour. `read` runs on mount and again by one timer a
 * second past `boundaryMs(value)`, so the caller re-renders at the boundary
 * and never with the 1 Hz frames. Pass module-level functions: the timer is
 * re-armed when the boundary or either function changes.
 */
export function useNextBoundary<T>(
  read: () => T,
  boundaryMs: (value: T) => number
): T {
  const [value, setValue] = useState(read);
  const at = boundaryMs(value);
  useEffect(() => {
    // A little past the boundary, so the new value is what `read` returns.
    const id = setTimeout(
      () => setValue(read()),
      Math.max(1000, at - Date.now() + 1000)
    );
    return () => clearTimeout(id);
  }, [at, read]);
  return value;
}
