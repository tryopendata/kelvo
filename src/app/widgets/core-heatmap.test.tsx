import { render } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import {
  CoreHeatmap,
  type CoreHeatmapProps,
  HeatScaleLegend,
} from "./core-heatmap";

const props: CoreHeatmapProps = {
  cores: [
    { id: "P0", cluster: "P", now: 34, buckets: [null, 20, 40, 34] },
    { id: "P1", cluster: "P", now: 22, buckets: [10, 15, 30, 22] },
    { id: "E0", cluster: "E", now: null, buckets: [52, 48, 50, null] },
  ],
  bucketMs: 10_000,
  windowMs: 40_000,
  ariaLabel: "Per-core load, last 40 seconds",
};

describe("CoreHeatmap", () => {
  it("renders the same after a JSON round trip", () => {
    expectJsonRoundTrip(CoreHeatmap, props);
  });

  it("is one img named by its aria-label", () => {
    const { getByRole } = render(<CoreHeatmap {...props} />);
    expect(
      getByRole("img", { name: "Per-core load, last 40 seconds" })
    ).toBeTruthy();
  });

  it("draws a gap band across the core rows on its bucket axis", () => {
    // Four 10 s columns, the newest starting at 30 s: the axis is 0 to 40 s.
    const { getByRole } = render(
      <CoreHeatmap
        {...props}
        endMs={30_000}
        gaps={[{ fromMs: 10_000, toMs: 20_000, label: "Asleep" }]}
      />
    );
    const band = getByRole("note", { name: "Asleep" });
    expect(band.style.left).toBe("25.00%");
    expect(band.style.width).toBe("25.00%");
    expect(band.parentElement?.style.gridRow).toBe("1 / 4");
  });

  it("hatches buckets with no samples instead of tinting them", () => {
    const { container } = render(<CoreHeatmap {...props} />);
    const gaps = container.querySelectorAll<HTMLElement>("[data-gap]");
    expect(gaps).toHaveLength(2);
    for (const g of gaps) {
      expect(g.style.background).toContain("repeating-linear-gradient");
    }
  });

  it("pads short rows on the left so now is the last column", () => {
    const { container } = render(
      <CoreHeatmap
        {...props}
        cores={[{ id: "P0", cluster: "P", now: 50, buckets: [50] }]}
      />
    );
    const cells = container.querySelectorAll<HTMLElement>(
      ".grid.gap-px > span"
    );
    expect(cells).toHaveLength(4);
    expect(cells[0]?.dataset.gap).toBe("true");
    expect(cells[3]?.dataset.gap).toBeUndefined();
  });

  it("keeps each closed cell's element as the window slides a column", () => {
    const row = (buckets: (number | null)[]) => [
      { id: "P0", cluster: "P" as const, now: 1, buckets },
    ];
    const cellsOf = (c: HTMLElement) => [
      ...c.querySelectorAll<HTMLElement>(".grid.gap-px > span"),
    ];
    const { container, rerender } = render(
      <CoreHeatmap {...props} cores={row([1, 2, 3, 4])} endMs={70_000} />
    );
    const before = cellsOf(container);
    // One column closed: buckets 5..8 become 6..9.
    rerender(
      <CoreHeatmap {...props} cores={row([2, 3, 4, 5])} endMs={80_000} />
    );
    const after = cellsOf(container);
    expect(after).toHaveLength(4);
    // Bucket 6 (was second) and 7 (was third) are the same elements.
    expect(after[0]).toBe(before[1]);
    expect(after[1]).toBe(before[2]);
  });

  it("shows a dash for a stale current value, not 0%", () => {
    const { getByText } = render(<CoreHeatmap {...props} />);
    expect(getByText("–")).toBeTruthy();
  });

  it("legend renders the four steps", () => {
    const { container } = render(<HeatScaleLegend />);
    expect(container.querySelectorAll(".rounded-mark")).toHaveLength(4);
  });
});
