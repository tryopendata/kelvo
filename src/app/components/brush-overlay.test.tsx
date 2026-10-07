import { formatClockSeconds } from "@core/format";
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Profiler } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "~/components/ui/dialog";
import { BrushProvider, useBrushRange } from "~/stores/brush-store";
import { BRUSH_SCOPE_ATTR } from "./brush-overlay";
import { LiveMirrorChart } from "./live-mirror-chart";

const T = 1_700_000_000_000; // a 10 s edge
const NOW = T + 5000;
const S = 1000;
const WIDTH = 600;

function Probe() {
  const r = useBrushRange();
  return (
    <output data-testid="range">
      {r ? `${r.fromMs - T}:${r.toMs - T}` : "none"}
    </output>
  );
}

const chart = (
  <LiveMirrorChart
    brush
    upKey="net.tx_total"
    downKey="net.rx_total"
    upLabel="Upload"
    downLabel="Download"
    windowMs={60_000}
    accent="net"
    format={String}
    minCeiling={1}
  />
);

/** The 1m chart spans [NOW − 59 s, NOW + 1 s): x px is that much along it. */
const xAt = (tMs: number) => ((tMs - (NOW - 59 * S)) / (60 * S)) * WIDTH;

async function setup(onRender?: () => void) {
  const utils = renderWithProviders(
    <BrushProvider>
      {onRender ? (
        <Profiler id="chart" onRender={onRender}>
          {chart}
        </Profiler>
      ) : (
        chart
      )}
      <Probe />
    </BrushProvider>,
    { transportOptions: { now: () => NOW } }
  );
  const brush = await screen.findByRole("slider", {
    name: "Select a time range",
  });
  brush.getBoundingClientRect = () =>
    ({ left: 0, top: 0, width: WIDTH, height: 193 }) as DOMRect;
  return { ...utils, brush };
}

const range = () => screen.getByTestId("range").textContent;

describe("chart brush (D-089)", () => {
  it("a drag selects the snapped range, drawn while dragging", async () => {
    const { brush } = await setup();
    fireEvent.pointerDown(brush, { button: 0, clientX: xAt(T - 24 * S) });
    fireEvent.pointerMove(brush, { clientX: xAt(T - 15 * S) });
    // Drawn, not committed: the band follows, the table does not query.
    expect(screen.getByTestId("brush-band")).toBeInTheDocument();
    expect(range()).toBe("none");
    fireEvent.pointerMove(brush, { clientX: xAt(T - 9 * S) });
    fireEvent.pointerUp(brush, { clientX: xAt(T - 9 * S) });
    expect(range()).toBe(`${-30 * S}:0`);
  });

  it("a click selects the 10 s bucket under the pointer", async () => {
    const { brush } = await setup();
    fireEvent.pointerDown(brush, { button: 0, clientX: xAt(T - 24 * S) });
    fireEvent.pointerUp(brush, { clientX: xAt(T - 24 * S) + 1 });
    expect(range()).toBe(`${-30 * S}:${-20 * S}`);
  });

  it("dims bars outside the selection", async () => {
    const { brush } = await setup();
    fireEvent.pointerDown(brush, { button: 0, clientX: xAt(T - 24 * S) });
    fireEvent.pointerUp(brush, { clientX: xAt(T - 24 * S) });
    const img = screen.getByRole("img", { name: /Drag to select a range/ });
    // 60 one-second bars per side; the 10 s selected stay at full strength.
    const bars = img.querySelectorAll("span");
    const dim = [...bars].filter((b) => b.classList.contains("opacity-35"));
    expect(bars.length).toBe(120);
    expect(dim.length).toBe(100);
  });

  it("keyboard: Enter selects the focused bucket, Shift+arrows extend, Esc clears", async () => {
    const { brush, user } = await setup();
    await user.tab();
    expect(brush).toHaveFocus();
    // Focus starts on the newest bucket that has fully happened: [T − 10 s, T).
    await user.keyboard("{Enter}");
    expect(range()).toBe(`${-10 * S}:0`);
    await user.keyboard("{Shift>}{ArrowLeft}{ArrowLeft}{/Shift}");
    expect(range()).toBe(`${-30 * S}:0`);
    // A plain arrow moves the focus only, and never past the newest bucket.
    await user.keyboard("{ArrowLeft}{ArrowRight}{ArrowRight}{ArrowRight}");
    expect(range()).toBe(`${-30 * S}:0`);
    await user.keyboard("{Enter}");
    expect(range()).toBe(`${-10 * S}:0`);
    await user.keyboard("{Escape}");
    expect(range()).toBe("none");
  });

  it("a click on the chart clears a selection rather than picking a bucket", async () => {
    const { brush } = await setup();
    fireEvent.pointerDown(brush, { button: 0, clientX: xAt(T - 24 * S) });
    fireEvent.pointerUp(brush, { clientX: xAt(T - 24 * S) });
    expect(range()).toBe(`${-30 * S}:${-20 * S}`);
    fireEvent.pointerDown(brush, { button: 0, clientX: xAt(T - 4 * S) });
    fireEvent.pointerUp(brush, { clientX: xAt(T - 4 * S) });
    expect(range()).toBe("none");
  });

  it("Esc and a press on empty space outside clear; presses inside a scope, on controls or typing do not (D-093)", async () => {
    renderWithProviders(
      <BrushProvider>
        <div {...{ [BRUSH_SCOPE_ATTR]: "" }}>
          {chart}
          <p data-testid="inside">table</p>
        </div>
        <p data-testid="outside">empty</p>
        <button type="button">Elsewhere</button>
        <input aria-label="Search" />
        <Probe />
      </BrushProvider>,
      { transportOptions: { now: () => NOW } }
    );
    const brush = await screen.findByRole("slider", {
      name: "Select a time range",
    });
    brush.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: WIDTH, height: 193 }) as DOMRect;
    const pick = () => {
      fireEvent.pointerDown(brush, { button: 0, clientX: xAt(T - 24 * S) });
      fireEvent.pointerUp(brush, { clientX: xAt(T - 24 * S) });
      expect(range()).not.toBe("none");
    };

    pick();
    fireEvent.pointerDown(screen.getByTestId("inside"), { button: 0 });
    fireEvent.pointerDown(screen.getByRole("button", { name: "Elsewhere" }), {
      button: 0,
    });
    fireEvent.pointerDown(screen.getByTestId("outside"), { button: 2 });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Search" }), {
      key: "Escape",
    });
    expect(range()).not.toBe("none");

    fireEvent.pointerDown(screen.getByTestId("outside"), { button: 0 });
    expect(range()).toBe("none");

    pick();
    fireEvent.keyDown(document.body, { key: "Escape" });
    expect(range()).toBe("none");
  });

  it("a press on an open dialog's backdrop keeps the selection", async () => {
    renderWithProviders(
      <BrushProvider>
        {chart}
        <p data-testid="outside">empty</p>
        <Probe />
        <Dialog defaultOpen>
          <DialogContent>
            <DialogTitle>Settings</DialogTitle>
            <DialogDescription>A dialog over the chart.</DialogDescription>
          </DialogContent>
        </Dialog>
      </BrushProvider>,
      { transportOptions: { now: () => NOW } }
    );
    // The open modal hides the chart from the accessibility tree.
    const brush = await screen.findByRole("slider", {
      name: "Select a time range",
      hidden: true,
    });
    brush.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: WIDTH, height: 193 }) as DOMRect;
    fireEvent.pointerDown(brush, { button: 0, clientX: xAt(T - 24 * S) });
    fireEvent.pointerUp(brush, { clientX: xAt(T - 24 * S) });
    expect(range()).not.toBe("none");

    // Radix portals the overlay beside the dialog content, not inside it.
    const overlay = document.querySelector("[data-slot=dialog-overlay]");
    expect(overlay).not.toBeNull();
    fireEvent.pointerDown(overlay as Element, { button: 0 });
    expect(range()).not.toBe("none");

    // Closed, the same press on empty space clears.
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument()
    );
    fireEvent.pointerDown(screen.getByTestId("outside"), { button: 0 });
    expect(range()).toBe("none");
  });

  it("selects no time after the newest elapsed 10 s edge", async () => {
    const { brush } = await setup();
    // [T, T + 6 s) is on the chart but the bucket has not finished.
    fireEvent.pointerDown(brush, { button: 0, clientX: xAt(T + 3 * S) });
    fireEvent.pointerUp(brush, { clientX: xAt(T + 3 * S) + 1 });
    expect(range()).toBe(`${-10 * S}:0`);
    fireEvent.pointerDown(brush, { button: 0, clientX: xAt(T - 24 * S) });
    fireEvent.pointerMove(brush, { clientX: xAt(T + 5 * S) });
    fireEvent.pointerUp(brush, { clientX: WIDTH });
    expect(range()).toBe(`${-30 * S}:0`);
  });

  it("announces the focused bucket, then the selection, and how to use it", async () => {
    const { brush, user } = await setup();
    const at = (ms: number) => formatClockSeconds(T + ms);
    expect(brush).toHaveAccessibleDescription(
      "Arrow keys move, Enter selects, Shift+arrows extend, Escape clears"
    );
    await user.tab();
    expect(brush).toHaveAttribute(
      "aria-valuetext",
      `${at(-10 * S)} to ${at(0)}, no selection`
    );
    await user.keyboard("{Enter}{Shift>}{ArrowLeft}{/Shift}{ArrowLeft}");
    expect(brush).toHaveAttribute(
      "aria-valuetext",
      `${at(-30 * S)} to ${at(-20 * S)}, selected ${at(-20 * S)} to ${at(0)}`
    );
    // Positions, not epoch ms: the chart's six whole buckets are 0 to 5.
    expect(brush).toHaveAttribute("aria-valuemin", "0");
    expect(brush).toHaveAttribute("aria-valuemax", "5");
    expect(brush).toHaveAttribute("aria-valuenow", "3");
  });

  it("pointer moves outside a drag re-render nothing, but move the hover column", async () => {
    const onRender = vi.fn();
    const { brush } = await setup(onRender);
    await act(async () => {});
    onRender.mockClear();
    for (const s of [40, 30, 20, 10, 0]) {
      fireEvent.pointerMove(brush, { clientX: xAt(T - s * S) });
    }
    expect(onRender).not.toHaveBeenCalled();
    const hover = screen.getByTestId("brush-hover");
    expect(hover.hidden).toBe(false);
    // At T the pointer is past the newest elapsed edge, so the column is
    // the last selectable bucket, [T − 10 s, T), on the [T − 54 s, T + 6 s)
    // chart.
    expect(hover.style.left).toBe("73.333%");
    expect(hover.style.width).toBe("16.667%");
  });
});
