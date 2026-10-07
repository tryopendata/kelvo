import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { BATTERY_STAT_STRIP, CPU_STAT_STRIP } from "./lib/sample-props";
import { StatStrip } from "./stat-strip";

describe("StatStrip", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(StatStrip, CPU_STAT_STRIP);
    expectJsonRoundTrip(StatStrip, BATTERY_STAT_STRIP);
  });

  it("renders the hero and the secondary figures", () => {
    render(<StatStrip {...CPU_STAT_STRIP} />);
    expect(screen.getByText("18%")).toBeInTheDocument();
    expect(screen.getByText("Load avg").nextElementSibling).toHaveTextContent(
      "3.42 2.98 2.71"
    );
  });
});
