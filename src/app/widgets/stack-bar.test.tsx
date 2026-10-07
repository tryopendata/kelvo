import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { MEMORY_STACK, POWER_STACK_BAR } from "./lib/sample-props";
import { StackBar } from "./stack-bar";

describe("StackBar", () => {
  it("round-trips its props through JSON", () => {
    expectJsonRoundTrip(StackBar, MEMORY_STACK);
    expectJsonRoundTrip(StackBar, POWER_STACK_BAR);
  });

  it("names every segment, including a zero-width one", () => {
    render(<StackBar {...POWER_STACK_BAR} />);
    expect(screen.getByRole("img").getAttribute("aria-label")).toContain(
      "ANE 0.0 W"
    );
  });

  it("leaves a missing segment out of the bar but keeps its legend row", () => {
    const segments = POWER_STACK_BAR.segments.map((s) =>
      s.key === "gpu" ? { ...s, value: "–", fraction: null } : s
    );
    render(<StackBar segments={segments} showLegend />);
    const bar = screen.getByRole("img");
    // CPU, DRAM, rest: GPU is missing and ANE is a measured 0.
    expect(bar.children).toHaveLength(3);
    expect(bar.querySelector("[data-missing]")).toBeNull();
    expect(screen.getByText("GPU")).toBeInTheDocument();
  });

  it("draws an empty track when nothing is measured", () => {
    const segments = POWER_STACK_BAR.segments.map((s) => ({
      ...s,
      value: "–",
      fraction: null,
    }));
    render(<StackBar segments={segments} />);
    const bar = screen.getByRole("img");
    expect(bar.children).toHaveLength(1);
    expect(bar.querySelector("[data-missing]")).toHaveClass("bg-track");
  });

  it("keeps the legend row of a zero-width segment", () => {
    render(<StackBar {...POWER_STACK_BAR} showLegend />);
    // ANE has no slice in the bar, but still has its legend key.
    expect(screen.getByRole("img").children).toHaveLength(4);
    expect(screen.getByText("ANE")).toBeInTheDocument();
  });
});
