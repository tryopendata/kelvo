import { act, renderHook } from "@testing-library/react";
import { useNextBoundary } from "./use-next-boundary";

const MIN = 60_000;
const minuteStart = () => Math.floor(Date.now() / MIN) * MIN;
const nextMinute = (start: number) => start + MIN;

describe("useNextBoundary", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 9, 4, 14, 2, 30));
  });
  afterEach(() => vi.useRealTimers());

  it("reads again a second past the boundary, not before", () => {
    const { result } = renderHook(() =>
      useNextBoundary(minuteStart, nextMinute)
    );
    const first = result.current;
    expect(first).toBe(new Date(2026, 9, 4, 14, 2).getTime());

    act(() => vi.advanceTimersByTime(30_000));
    expect(result.current).toBe(first);

    act(() => vi.advanceTimersByTime(1000));
    expect(result.current).toBe(first + MIN);

    act(() => vi.advanceTimersByTime(MIN));
    expect(result.current).toBe(first + 2 * MIN);
  });
});
