import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { CoreTiles } from "./core-tiles";
import { CORE_TILES } from "./lib/sample-props";

describe("CoreTiles", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(CoreTiles, CORE_TILES);
  });

  it("prints every core's load and names it", () => {
    render(<CoreTiles {...CORE_TILES} />);
    expect(screen.getByLabelText("P2 41%")).toHaveTextContent("41");
    expect(screen.getAllByRole("listitem")).toHaveLength(14);
  });

  it("shows a missing sample as a gap, not zero", () => {
    render(
      <CoreTiles
        clusters={[
          {
            id: "E0",
            name: "E-cluster",
            freq: "–",
            cores: [{ id: "E0", load: null }],
          },
        ]}
      />
    );
    const tile = screen.getByLabelText("E0 no sample");
    expect(tile).toHaveTextContent("–");
    expect(tile).not.toHaveTextContent("0");
  });
});
