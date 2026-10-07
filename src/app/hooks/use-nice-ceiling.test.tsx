import { renderHook } from "@testing-library/react";
import { useNiceCeiling } from "./use-nice-ceiling";

type Props = { values: number[]; nowMs: number; windowMs: number };

function setup(initial: Props) {
  return renderHook(
    ({ values, nowMs, windowMs }: Props) =>
      useNiceCeiling(values, nowMs, 1, windowMs),
    { initialProps: initial }
  );
}

describe("useNiceCeiling", () => {
  it("holds a higher ceiling for 60 s while the window stays the same", () => {
    const { result, rerender } = setup({
      values: [19],
      nowMs: 1000,
      windowMs: 300_000,
    });
    expect(result.current).toBe(20);
    rerender({ values: [3], nowMs: 2000, windowMs: 300_000 });
    expect(result.current).toBe(20);
  });

  it("fits the new window's data at once when the window changes", () => {
    const { result, rerender } = setup({
      values: [19],
      nowMs: 1000,
      windowMs: 300_000,
    });
    rerender({ values: [3], nowMs: 2000, windowMs: 900_000 });
    expect(result.current).toBe(4);
  });

  it("refits on a window change even before the next tick", () => {
    const { result, rerender } = setup({
      values: [19],
      nowMs: 1000,
      windowMs: 300_000,
    });
    rerender({ values: [3], nowMs: 1000, windowMs: 900_000 });
    expect(result.current).toBe(4);
  });
});
