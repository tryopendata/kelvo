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

  it("stripes a dashed item's swatch and leaves the others solid", () => {
    const { container } = render(
      <Legend
        items={[
          { label: "Solid", value: "1 W", step: 2 },
          { label: "Dashed", value: "2 W", step: 2, dashed: true },
        ]}
      />
    );
    const [solid, dashed] = container.querySelectorAll<HTMLElement>(
      "li > span[aria-hidden]"
    );
    // jsdom drops a gradient over color-mix(), so the variant is read from
    // its marker; the stripe itself is checked by screenshot.
    expect(solid).not.toHaveAttribute("data-dashed");
    expect(dashed).toHaveAttribute("data-dashed");
  });

  it("shows the label alone for an item without a value", () => {
    render(<Legend items={[{ label: "System", step: 1 }]} />);
    const item = screen.getByRole("listitem");
    expect(item).toHaveTextContent(/^System$/);
    expect(item.querySelector(".figures")).toBeNull();
  });
});
