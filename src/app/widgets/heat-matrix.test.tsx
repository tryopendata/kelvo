import { MISSING } from "@core/format";
import { render } from "@testing-library/react";
import {
  HeatMatrix,
  type HeatMatrixProps,
  HeatScaleLegend,
} from "./heat-matrix";

const props: HeatMatrixProps = {
  rows: [
    {
      id: "a",
      label: "Zone 01",
      title: "PMU tdie1",
      group: "x",
      now: 65,
      buckets: [20, 65, 110, 65],
    },
    { id: "b", label: "Zone 02", group: "x", now: 40, buckets: [40] },
    { id: "c", label: "Zone 03", group: "y", now: null, buckets: [50] },
  ],
  scale: { min: 20, max: 110 },
  formatNow: (v) => `${Math.round(v)}°`,
  bucketMs: 10_000,
  windowMs: 40_000,
  accent: "temp",
  ariaLabel: "SoC thermal zones",
};

const cellsOf = (c: HTMLElement) => [
  ...c.querySelectorAll<HTMLElement>(".grid.gap-px > span"),
];

/** A cell's accent mix, "51%". */
const tint = (c: HTMLElement | undefined) => c?.style.getPropertyValue("--t");

describe("HeatMatrix", () => {
  it("tints cells by their place on the scale, not from zero", () => {
    const { container } = render(<HeatMatrix {...props} />);
    const row = cellsOf(container).slice(0, 4);
    // 20 °C is the scale floor (6%), 65 °C its middle, 110 °C the top (95%).
    expect(row.map(tint)).toEqual(["6%", "51%", "95%", "51%"]);
  });

  it("formats the current value with formatNow and dashes a stale one", () => {
    const { getByText } = render(<HeatMatrix {...props} />);
    expect(getByText("65°")).toBeTruthy();
    expect(getByText("40°")).toBeTruthy();
    expect(getByText(MISSING)).toBeTruthy();
  });

  it("puts the row title on its label", () => {
    const { getByText } = render(<HeatMatrix {...props} />);
    expect(getByText("Zone 01").title).toBe("PMU tdie1");
  });

  it("breaks rows only where the group changes", () => {
    const { getByText } = render(<HeatMatrix {...props} />);
    const spaced = (label: string) =>
      getByText(label).parentElement?.className.includes("[&>*]:mt-2");
    expect(spaced("Zone 01")).toBe(false);
    expect(spaced("Zone 02")).toBe(false);
    expect(spaced("Zone 03")).toBe(true);
  });

  it("sizes the label column from labelWidth", () => {
    const { getByRole } = render(<HeatMatrix {...props} labelWidth={44} />);
    expect(
      getByRole("img", { name: "SoC thermal zones" }).style.gridTemplateColumns
    ).toBe("44px minmax(0, 1fr) 40px");
  });

  it("re-tints the closed cells when the scale changes", () => {
    const { container, rerender } = render(<HeatMatrix {...props} />);
    expect(tint(cellsOf(container)[0])).toBe("6%");
    rerender(<HeatMatrix {...props} scale={{ min: 0, max: 20 }} />);
    expect(tint(cellsOf(container)[0])).toBe("95%");
  });

  it("legend shows the given end labels", () => {
    const { getByText } = render(
      <HeatScaleLegend accent="temp" minLabel="68 °F" maxLabel="230 °F" />
    );
    expect(getByText("68 °F")).toBeTruthy();
    expect(getByText("230 °F")).toBeTruthy();
  });
});
