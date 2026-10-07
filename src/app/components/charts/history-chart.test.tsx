import { render } from "@testing-library/react";
import {
  HistoryChart,
  type HistoryChartProps,
  historyColumns,
} from "./history-chart";

// happy-dom has no 2D canvas or Path2D. uPlot only needs their methods to
// exist; every call is a no-op here.
class FakePath2D {
  moveTo() {}
  lineTo() {}
  rect() {}
  arc() {}
  bezierCurveTo() {}
  closePath() {}
  addPath() {}
}

// uPlot draws in a queued microtask, so the stubs stay up for the whole file.
beforeAll(() => {
  vi.stubGlobal("Path2D", FakePath2D);
});
afterAll(async () => {
  await new Promise((resolve) => setTimeout(resolve, 0));
  vi.unstubAllGlobals();
});

function stubCanvas() {
  const ctx = new Proxy(
    {},
    {
      get: (_t, prop) =>
        prop === "measureText" ? () => ({ width: 0 }) : () => undefined,
      set: () => true,
    }
  );
  return vi
    .spyOn(HTMLCanvasElement.prototype, "getContext")
    .mockImplementation(() => ctx as unknown as CanvasRenderingContext2D);
}

const props: HistoryChartProps = {
  points: [0, 1, 2, 3, 4, 5].map((i) => ({
    t: i * 60_000,
    min: 10 + i,
    max: 30 + i,
    avg: 20 + i,
  })),
  gaps: [
    { fromMs: 120_000, toMs: 240_000, label: "Asleep · not interpolated" },
  ],
  range: { fromMs: 0, toMs: 300_000 },
  domain: [0, 100],
  accent: "cpu",
  height: 72,
  ariaLabel: "CPU, last hour",
};

describe("historyColumns", () => {
  it("drops points inside a gap and puts a null row at its start", () => {
    const [xs, max, min, avg] = historyColumns(props.points, props.gaps);
    expect(xs).toEqual([0, 60_000, 120_000, 240_000, 300_000]);
    expect(avg[2]).toBeNull();
    expect(min[2]).toBeNull();
    expect(max[2]).toBeNull();
    expect(avg[3]).toBe(24);
  });
});

describe("HistoryChart", () => {
  it("mounts a uPlot instance and destroys it on unmount", () => {
    const getContext = stubCanvas();
    const removeListener = vi.spyOn(window, "removeEventListener");
    const { container, getByRole, unmount } = render(
      <HistoryChart {...props} />
    );
    expect(getByRole("img", { name: "CPU, last hour" })).toBeTruthy();
    expect(container.querySelector(".uplot")).not.toBeNull();
    expect(getByRole("note", { name: props.gaps[0]?.label })).toBeTruthy();

    unmount();
    // uPlot.destroy() unhooks its device-pixel-ratio listener from window.
    expect(
      removeListener.mock.calls.some(([event]) => event === "dppxchange")
    ).toBe(true);
    removeListener.mockRestore();
    return new Promise<void>((resolve) =>
      setTimeout(() => {
        getContext.mockRestore();
        resolve();
      }, 0)
    );
  });
});
