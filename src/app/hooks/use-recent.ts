import { useState } from "react";

/**
 * The last `size` values of `value`, oldest first, one per tick: a new `tick`
 * (the live frame's timestamp) appends the current value, a repeat or a gap
 * (`null`, which breaks the sparkline) included. The onboarding "Graph per
 * module" preview draws its sparkline from it, as the Rust tray keeps its own
 * ring rather than querying history.
 */
export function useRecent(
  value: number | null,
  tick: number | null,
  size: number
): readonly (number | null)[] {
  const [state, setState] = useState<{
    tick: number | null;
    recent: readonly (number | null)[];
  }>({ tick: null, recent: [] });
  // Adjusted during render rather than in an effect, so the new sample shows
  // in the same commit as the tick that brought it.
  if (tick !== null && tick !== state.tick) {
    const recent = [...state.recent.slice(-(size - 1)), value];
    setState({ tick, recent });
    return recent;
  }
  return state.recent;
}
