import { render } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { RING_GAUGE } from "./lib/sample-props";
import { RingGauge } from "./ring-gauge";

describe("RingGauge", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(RingGauge, RING_GAUGE);
  });

  it("starts the second segment where the first ends", () => {
    const { container } = render(<RingGauge {...RING_GAUGE} />);
    const arcs = container.querySelectorAll("circle");
    // Track plus two segments.
    expect(arcs).toHaveLength(3);
    const angle = Number(
      /rotate\(([\d.]+)deg\)/.exec(arcs[2]?.getAttribute("style") ?? "")?.[1]
    );
    expect(angle).toBeCloseTo(0.124 * 360, 0);
  });

  it("clamps the total to one full ring", () => {
    const { container } = render(
      <RingGauge {...RING_GAUGE} fractions={[0.8, 0.7]} />
    );
    const [, first, second] = Array.from(container.querySelectorAll("circle"));
    const c = 2 * Math.PI * 30;
    const shown = (el: Element | undefined) =>
      c - Number(el?.getAttribute("stroke-dashoffset"));
    expect(shown(first) + shown(second)).toBeCloseTo(c, 0);
  });
});
