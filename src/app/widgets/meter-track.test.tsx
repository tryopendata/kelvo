import { render } from "@testing-library/react";
import { MeterTrack } from "./meter-track";

describe("MeterTrack", () => {
  it("draws a missing value as an empty track, not a 0 fill", () => {
    const { container } = render(<MeterTrack fraction={null} />);
    expect(container.querySelector("[data-missing]")).toHaveClass("bg-track");
    expect(container.querySelector("[style*=scaleX]")).toBeNull();
  });

  it("is hidden from assistive tech: the figure beside it carries the value", () => {
    const { container } = render(<MeterTrack fraction={0.5} />);
    expect(container.firstElementChild).toHaveAttribute("aria-hidden", "true");
  });

  it("clamps the fill and does not tween unless asked", () => {
    const { container, rerender } = render(<MeterTrack fraction={1.4} />);
    const fill = container.querySelector<HTMLElement>("[style*=scaleX]");
    expect(fill?.style.transform).toBe("scaleX(1)");
    expect(fill?.className).not.toContain("transition-transform");

    rerender(<MeterTrack fraction={0.5} transition />);
    expect(
      container.querySelector<HTMLElement>("[style*=scaleX]")?.className
    ).toContain("transition-transform");
  });
});
