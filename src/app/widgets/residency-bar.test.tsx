import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { RESIDENCY } from "./lib/sample-props";
import { ResidencyBar, type ResidencyBarProps } from "./residency-bar";

const P = RESIDENCY[0] as ResidencyBarProps;

describe("ResidencyBar", () => {
  it("round-trips its props through JSON", () => {
    for (const r of RESIDENCY) expectJsonRoundTrip(ResidencyBar, r);
  });

  it("names each state with its share", () => {
    render(<ResidencyBar {...P} />);
    expect(screen.getByRole("img").getAttribute("aria-label")).toBe(
      "P-cluster residency: 4.51 GHz 4%, 3.86 GHz 9%, 3.20 GHz 18%, 2.42 GHz 10%, idle 59%"
    );
  });

  it("draws idle as the track, not in the accent", () => {
    render(<ResidencyBar {...P} />);
    const slices = screen.getByRole("img").querySelectorAll("span");
    expect(slices[slices.length - 1]?.style.background).toBe(
      "var(--color-track)"
    );
  });
});
