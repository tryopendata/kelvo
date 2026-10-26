import { createMockTransport } from "@core/mock-transport";
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { useTimelineData } from "../_hooks/use-timeline-data";
import { LANES } from "../_lib/lanes";
import { LanesCard } from "./lanes-card";

const NOW = 1_800_000_000_000;
const CPU = LANES.filter((lane) => lane.id === "cpu");
function Investigation() {
  const [view, setView] = useState({ windowMs: 600_000, endMs: NOW });
  const [saved, setSaved] = useState(0);
  const data = useTimelineData(view, CPU);
  return (
    <>
      <p>Selections {saved}</p>
      {data.error && (
        <button type="button" disabled={data.loading} onClick={data.retry}>
          Retry recorded detail
        </button>
      )}
      <LanesCard
        {...data}
        lanes={[]}
        events={[]}
        span="1h"
        live={false}
        units={{ rate: "MBps", temperature: "C" }}
        failed={data.error !== null}
        onSelectRange={(range) => {
          setSaved((value) => value + 1);
          setView({ windowMs: range.toMs - range.fromMs, endMs: range.toMs });
        }}
      />
    </>
  );
}
it("releases failed zoom controls, retries recorded detail, and accepts another keyboard and pointer selection", async () => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  const previous = Object.getOwnPropertyDescriptor(
    Element.prototype,
    "animate"
  );
  Object.defineProperty(Element.prototype, "animate", {
    configurable: true,
    value: () => ({ finished: Promise.resolve(), cancel: () => {} }),
  });
  document.documentElement.style.setProperty("--motion-enter", "280ms");
  const transport = createMockTransport({ now: () => NOW, autoTick: false });
  const read = transport.queryHistory.bind(transport);
  let fail = true;
  let release: (() => void) | undefined;
  const query = vi
    .spyOn(transport, "queryHistory")
    .mockImplementation(async (request) => {
      if (request.to_ms !== NOW && fail) {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
        return {
          status: "error",
          error: { kind: "remote_unreachable", message: "offline" },
        };
      }
      return read(request);
    });
  try {
    const { user } = renderWithProviders(<Investigation />, { transport });
    const slider = await screen.findByRole("slider", {
      name: "Timeline cursor",
    });
    await waitFor(() => expect(slider).toHaveAttribute("aria-busy", "false"));
    const select = () => {
      fireEvent.focus(slider);
      fireEvent.keyDown(slider, { key: "Home" });
      for (let i = 0; i < 6; i++)
        fireEvent.keyDown(slider, { key: "ArrowRight", shiftKey: true });
      fireEvent.keyDown(slider, { key: "Enter" });
    };
    select();
    await screen.findByText(/Loading recorded detail/);
    await waitFor(() => expect(release).toBeDefined());
    await act(async () => release?.());
    const retry = await screen.findByRole("button", {
      name: "Retry recorded detail",
    });
    await waitFor(() =>
      expect(
        screen.queryByText(/Loading recorded detail/)
      ).not.toBeInTheDocument()
    );
    expect(slider.style.pointerEvents).not.toBe("none");
    fireEvent.keyDown(slider, { key: "Home" });
    fireEvent.keyDown(slider, { key: "ArrowRight" });
    expect(slider).not.toHaveAttribute("aria-valuetext", "No time selected");
    slider.setPointerCapture = vi.fn();
    slider.hasPointerCapture = () => false;
    fireEvent.pointerDown(slider, { button: 0, pointerId: 1, clientX: 0 });
    fireEvent.pointerMove(slider, { pointerId: 1, clientX: 320 });
    fireEvent.pointerUp(slider, { pointerId: 1, clientX: 320 });
    expect(screen.getByText("Selections 2")).toBeVisible();
    fail = false;
    await user.click(retry);
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "Retry recorded detail" })
      ).not.toBeInTheDocument()
    );
    expect(
      query.mock.calls.filter(([request]) => request.to_ms !== NOW)
    ).toHaveLength(2);
    select();
    expect(screen.getByText("Selections 3")).toBeVisible();
    await waitFor(() => expect(slider).toHaveAttribute("aria-busy", "false"));
  } finally {
    document.documentElement.style.removeProperty("--motion-enter");
    if (previous) Object.defineProperty(Element.prototype, "animate", previous);
    else Reflect.deleteProperty(Element.prototype, "animate");
    vi.useRealTimers();
  }
});
