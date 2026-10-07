import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { InlineBar, type InlineBarProps } from "./inline-bar";
import { INLINE_BAR, POPOVER_CPU_ROWS } from "./lib/sample-props";

describe("InlineBar", () => {
  it("round-trips its props through JSON in every layout", () => {
    expectJsonRoundTrip(InlineBar, INLINE_BAR);
    expectJsonRoundTrip(InlineBar, POPOVER_CPU_ROWS[0] as InlineBarProps);
    expectJsonRoundTrip(InlineBar, { ...INLINE_BAR, layout: "wide" });
    expectJsonRoundTrip(InlineBar, { ...INLINE_BAR, fraction: null });
    expectJsonRoundTrip(InlineBar, { ...INLINE_BAR, fraction: "none" });
  });

  it("clamps the fill to the track", () => {
    const { container } = render(<InlineBar {...INLINE_BAR} fraction={1.7} />);
    const fill = container.querySelector<HTMLElement>("[style*=scaleX]");
    expect(fill?.style.transform).toBe("scaleX(1)");
  });

  it("draws no bar when the fraction has no meaning", () => {
    const { container } = render(
      <InlineBar label="Fans" value="Passive cooling" fraction="none" />
    );
    expect(screen.getByText("Passive cooling")).toBeInTheDocument();
    expect(container.querySelector(".bg-track")).toBeNull();
    expect(container.querySelector("[style*=scaleX]")).toBeNull();
  });

  it.each(["stacked", "row", "wide"] as const)(
    "draws a missing value as an empty track, not a 0 fill (%s)",
    (layout) => {
      const { container } = render(
        <InlineBar label="User" value="–" fraction={null} layout={layout} />
      );
      expect(container.querySelector("[data-missing]")).toHaveClass("bg-track");
      expect(container.querySelector("[style*=scaleX]")).toBeNull();
      expect(screen.getByText("–")).toHaveClass("text-muted-foreground");
    }
  );

  it("draws a measured 0 as a fill", () => {
    const { container } = render(
      <InlineBar label="User" value="0%" fraction={0} layout="row" />
    );
    expect(container.querySelector("[data-missing]")).toBeNull();
    const fill = container.querySelector<HTMLElement>("[style*=scaleX]");
    expect(fill?.style.transform).toBe("scaleX(0)");
  });
});
