import { render } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { MirrorBars, type MirrorBarsProps } from "./mirror-bars";

const props: MirrorBarsProps = {
  up: [2, 6, null, 12],
  down: [4, 26, null, 10],
  intervalMs: 1000,
  tEndMs: 1_760_000_000_000,
  accent: "net",
  ariaLabel: "Upload above the line, download below, last 4 seconds",
};

describe("MirrorBars", () => {
  it("renders the same after a JSON round trip", () => {
    expectJsonRoundTrip(MirrorBars, props);
  });

  it("names the chart and its directions in the aria-label", () => {
    const { getByRole } = render(<MirrorBars {...props} />);
    expect(getByRole("img").getAttribute("aria-label")).toMatch(
      /Upload above.*download below/
    );
  });

  it("leaves a missing sample as an empty gap slot on both sides", () => {
    const { container } = render(<MirrorBars {...props} />);
    expect(container.querySelectorAll("[data-gap]")).toHaveLength(2);
  });

  it("scales each side to its own max by default", () => {
    const { container } = render(<MirrorBars {...props} />);
    const heights = [...container.querySelectorAll("span")].map(
      (s) => (s as HTMLElement).style.height
    );
    expect(heights).toContain("18px"); // up max 12 fills the 18 px half
    expect(heights).toContain("30px"); // down max 26 fills the 30 px half
  });
});
