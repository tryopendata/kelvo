import { render } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { WindowTicks } from "./window-ticks";

describe("WindowTicks", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(WindowTicks, { ticks: ["−60s", "−30s", "now"] });
  });

  it("brightens only the last label", () => {
    const { container } = render(<WindowTicks ticks={["−1s", "−1s", "now"]} />);
    const spans = [...container.querySelectorAll("span")];
    expect(spans.map((s) => s.textContent)).toEqual(["−1s", "−1s", "now"]);
    expect(
      spans.map((s) => s.className.includes("text-muted-foreground"))
    ).toEqual([false, false, true]);
  });
});
