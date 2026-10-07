import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import {
  BATTERY_STAT_GRID,
  GPU_STAT_GRID,
  POWER_STAT_GRID,
} from "./lib/sample-props";
import { StatGrid } from "./stat-grid";

describe("StatGrid", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(StatGrid, GPU_STAT_GRID);
    expectJsonRoundTrip(StatGrid, POWER_STAT_GRID);
    expectJsonRoundTrip(StatGrid, BATTERY_STAT_GRID);
  });

  it("pairs each label with its value", () => {
    render(<StatGrid {...GPU_STAT_GRID} />);
    expect(screen.getByText("Power").nextElementSibling).toHaveTextContent(
      "3.1 W"
    );
  });
});
