import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { useTimelineSelection } from "./use-timeline-selection";

function Probe({ from = 0, to = 300_000, rangeKey = "host:live:5m" }) {
  const [saved, setSaved] = useState("none");
  const selection = useTimelineSelection({
    fromMs: from,
    toMs: to,
    rangeKey,
    bucketMs: 10_000,
    width: 100,
    nowMs: 600_000,
    retainedFromMs: 0,
    loading: false,
    onSelectRange: (range) => setSaved(`${range.fromMs}-${range.toMs}`),
  });
  return (
    <>
      <div
        ref={selection.sliderRef}
        role="slider"
        aria-valuenow={selection.cursor ?? 0}
        aria-label="Recorded plot"
        tabIndex={0}
        {...selection.sliderProps}
      />
      <p>Saved: {saved}</p>
      <p>{selection.selecting ? "Selecting" : "Inspecting"}</p>
    </>
  );
}
function geometry() {
  const plot = screen.getByRole("slider", { name: "Recorded plot" });
  vi.spyOn(plot, "getBoundingClientRect").mockReturnValue({
    left: 0,
    width: 100,
  } as DOMRect);
  plot.setPointerCapture = vi.fn();
  plot.hasPointerCapture = () => false;
  return plot;
}
describe("selection during live bucket advancement", () => {
  it("releases a drag against its original dates after a live tail update", () => {
    const view = render(<Probe />);
    const plot = geometry();
    fireEvent.pointerDown(plot, { button: 0, pointerId: 1, clientX: 20 });
    fireEvent.pointerMove(plot, { pointerId: 1, clientX: 60 });
    expect(screen.getByText("Selecting")).toBeInTheDocument();
    view.rerender(<Probe from={10_000} to={310_000} />);
    fireEvent.pointerUp(plot, { pointerId: 1, clientX: 60 });
    expect(screen.getByText("Saved: 60000-180000")).toBeInTheDocument();
  });
  it("keeps a keyboard selection through tail advancement and cancels on a different committed range", () => {
    const view = render(<Probe />);
    const plot = geometry();
    fireEvent.focus(plot);
    fireEvent.keyDown(plot, { key: "Home" });
    fireEvent.keyDown(plot, { key: "ArrowRight", shiftKey: true });
    view.rerender(<Probe from={10_000} to={310_000} />);
    for (let i = 0; i < 5; i++)
      fireEvent.keyDown(plot, { key: "ArrowRight", shiftKey: true });
    fireEvent.keyDown(plot, { key: "Enter" });
    expect(screen.getByText("Saved: 0-60000")).toBeInTheDocument();
    fireEvent.keyDown(plot, { key: "ArrowRight", shiftKey: true });
    view.rerender(
      <Probe from={10_000} to={310_000} rangeKey="host:fixed:1h" />
    );
    expect(screen.getByText("Inspecting")).toBeInTheDocument();
    fireEvent.keyDown(plot, { key: "Enter" });
    expect(screen.getByText("Saved: 0-60000")).toBeInTheDocument();
  });
});
