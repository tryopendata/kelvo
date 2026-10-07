import { render } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { PowerStack, type PowerStackProps } from "./power-stack";

const T = 1_760_000_000_000;

const props: PowerStackProps = {
  series: [
    { key: "cpu", values: [6, 6.4, 7, 6.4] },
    { key: "gpu", values: [3, 3.1, 2.9, 3.1] },
    { key: "ane", values: [0, 1.4, 0, 0] },
    { key: "dram", values: [0.9, 0.9, 1, 0.9] },
  ],
  intervalMs: 5000,
  tEndMs: T,
  yMax: 20,
  annotations: [
    { tsMs: T - 10_000, label: "ANE 1.4 W · Photos face analysis" },
  ],
};

describe("PowerStack", () => {
  it("renders the same after a JSON round trip", () => {
    expectJsonRoundTrip(PowerStack, props);
  });

  it("places gap bands on its own time axis", () => {
    // Four values 5 s apart: the axis is the last 15 s.
    const { getByRole } = render(
      <PowerStack
        {...props}
        gaps={[{ fromMs: T - 15_000, toMs: T - 10_000, label: "Paused" }]}
      />
    );
    const band = getByRole("note", { name: "Paused" });
    expect(band.style.left).toBe("0.00%");
    expect(band.style.width).toBe("33.33%");
  });

  it("is an img with a default aria-label", () => {
    const { getByRole } = render(<PowerStack {...props} />);
    expect(
      getByRole("img", { name: "Stacked power: CPU, GPU, ANE, DRAM" })
    ).toBeTruthy();
  });

  it("fills ANE with the hatch pattern", () => {
    const { container } = render(<PowerStack {...props} />);
    const ane = container.querySelector<SVGPathElement>(
      '[data-component="ane"]'
    );
    expect(ane?.style.fill).toMatch(/^url\(#/);
  });

  it("breaks the whole stack when one component is missing", () => {
    const { container } = render(
      <PowerStack
        {...props}
        series={props.series.map((s) =>
          s.key === "gpu" ? { ...s, values: [3, null, 2.9, 3.1] } : s
        )}
      />
    );
    const top = container.querySelector("[data-line]")?.getAttribute("d") ?? "";
    expect(top.match(/M/g)).toHaveLength(2);
  });

  it("renders with no samples yet without duplicate-key warnings", () => {
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <PowerStack {...props} series={[]} intervalMs={1000} annotations={[]} />
    );
    expect(err).not.toHaveBeenCalled();
    err.mockRestore();
  });

  it("places annotations inside the window and drops ones outside it", () => {
    const { getByText, queryByText, rerender } = render(
      <PowerStack {...props} />
    );
    expect(getByText("ANE 1.4 W · Photos face analysis")).toBeTruthy();
    rerender(
      <PowerStack
        {...props}
        annotations={[{ tsMs: T - 3_600_000, label: "old" }]}
      />
    );
    expect(queryByText("old")).toBeNull();
  });
});
