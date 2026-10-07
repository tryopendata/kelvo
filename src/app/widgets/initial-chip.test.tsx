import { render } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { InitialChip } from "./initial-chip";

describe("InitialChip", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(InitialChip, { text: "X" });
  });

  it("shows one character", () => {
    const { container } = render(<InitialChip text="Xcode" />);
    expect(container.textContent).toBe("X");
  });
});
