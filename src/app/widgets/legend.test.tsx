import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { Legend } from "./legend";
import { LEGEND } from "./lib/sample-props";

describe("Legend", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(Legend, LEGEND);
    expectJsonRoundTrip(Legend, { ...LEGEND, columns: 2 });
  });

  it("names every series in text, not only by swatch", () => {
    render(<Legend {...LEGEND} />);
    expect(screen.getByText("User")).toBeInTheDocument();
    expect(screen.getByText("5.6%")).toBeInTheDocument();
  });
});
