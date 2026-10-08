import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useRecent } from "./use-recent";

interface Props {
  value: number | null;
  tick: number | null;
}

describe("useRecent", () => {
  it("appends one sample per tick, repeats and gaps included", () => {
    const initialProps: Props = { value: 10, tick: null };
    const { result, rerender } = renderHook(
      ({ value, tick }: Props) => useRecent(value, tick, 3),
      { initialProps }
    );
    expect(result.current).toEqual([]);

    rerender({ value: 10, tick: 1000 });
    // The same value on the next tick is a sample of its own.
    rerender({ value: 10, tick: 2000 });
    expect(result.current).toEqual([10, 10]);

    // A gap is kept, so the sparkline breaks there.
    rerender({ value: null, tick: 3000 });
    expect(result.current).toEqual([10, 10, null]);

    // No new tick, no new sample.
    rerender({ value: 40, tick: 3000 });
    expect(result.current).toEqual([10, 10, null]);

    // Bounded to the newest `size`.
    rerender({ value: 40, tick: 4000 });
    expect(result.current).toEqual([10, null, 40]);
  });
});
