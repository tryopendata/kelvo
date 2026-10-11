import { act, render, screen } from "@testing-library/react";
import { HostClockContext } from "~/lib/host-clock";
import { useNow } from "./use-now";

function ClockLabel() {
  return <span>{useNow(60000)}</span>;
}

describe("host clock label cadence", () => {
  afterEach(() => vi.useRealTimers());
  it("keeps its minute timer when connection polling updates the source clock", () => {
    vi.useFakeTimers();
    let sourceTime = 1000000;
    const { rerender } = render(
      <HostClockContext.Provider value={() => sourceTime}>
        <ClockLabel />
      </HostClockContext.Provider>
    );
    for (let i = 0; i < 12; i++) {
      sourceTime += 5000;
      act(() => vi.advanceTimersByTime(5000));
      rerender(
        <HostClockContext.Provider value={() => sourceTime}>
          <ClockLabel />
        </HostClockContext.Provider>
      );
    }
    expect(screen.getByText("1060000")).toBeInTheDocument();
  });
});
