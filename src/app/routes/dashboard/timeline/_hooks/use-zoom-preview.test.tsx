import type { TimeRange } from "@core/brush";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { useRef, useState } from "react";
import { useZoomPreview } from "./use-zoom-preview";

const SOURCE: TimeRange = { fromMs: 0, toMs: 600_000 };
const TARGET: TimeRange = { fromMs: 120_000, toMs: 240_000 };
function Probe() {
  const plot = useRef<HTMLDivElement>(null);
  const [bounds, setBounds] = useState(SOURCE);
  const [loading, setLoading] = useState(false);
  const zoom = useZoomPreview(plot, bounds, "1 min avg", loading, "host-a");
  return (
    <>
      <div ref={plot}>
        <div data-lane-content>Coarse recorded plot</div>
        <div ref={zoom.overlayRef} />
      </div>
      <p>Range ends {bounds.toMs}</p>
      {zoom.preview && (
        <p role="status">Preview at {zoom.preview.resolution}</p>
      )}
      <button
        type="button"
        onClick={() => {
          zoom.begin(TARGET);
          setBounds(TARGET);
          setLoading(true);
        }}
      >
        Zoom
      </button>
      <button
        type="button"
        onClick={() => setBounds({ fromMs: 300_000, toMs: 600_000 })}
      >
        Another range
      </button>
      <button type="button" onClick={() => setLoading(false)}>
        Old response
      </button>
    </>
  );
}

it("removes a superseded projection immediately and never restores it for a late read", async () => {
  const previous = Object.getOwnPropertyDescriptor(
    Element.prototype,
    "animate"
  );
  const animations: Array<() => void> = [];
  Object.defineProperty(Element.prototype, "animate", {
    configurable: true,
    value: () => {
      let finish = () => {};
      const finished = new Promise<void>((resolve) => {
        finish = resolve;
      });
      animations.push(finish);
      return { finished, cancel: finish };
    },
  });
  document.documentElement.style.setProperty("--motion-enter", "280ms");
  try {
    render(<Probe />);
    fireEvent.click(screen.getByRole("button", { name: "Zoom" }));
    expect(screen.getByRole("status")).toHaveTextContent(
      "Preview at 1 min avg"
    );
    fireEvent.click(screen.getByRole("button", { name: "Another range" }));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.getByText("Range ends 600000")).toBeVisible();
    await act(async () => {
      for (const finish of animations) finish();
    });
    fireEvent.click(screen.getByRole("button", { name: "Old response" }));
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  } finally {
    document.documentElement.style.removeProperty("--motion-enter");
    if (previous) Object.defineProperty(Element.prototype, "animate", previous);
    else Reflect.deleteProperty(Element.prototype, "animate");
  }
});
