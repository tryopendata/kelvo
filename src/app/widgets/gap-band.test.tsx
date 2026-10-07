import { render } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { GapBand, type GapBandProps, GapBands } from "./gap-band";

const props: GapBandProps = {
  fromMs: 1_000,
  toMs: 2_000,
  label: "Asleep 11:02–11:31 · not interpolated",
  rangeFromMs: 0,
  rangeToMs: 4_000,
};

describe("GapBand", () => {
  it("renders the same after a JSON round trip", () => {
    expectJsonRoundTrip(GapBand, props);
  });

  it("positions itself on the chart's range", () => {
    const { getByRole } = render(<GapBand {...props} />);
    const band = getByRole("note", { name: props.label });
    expect(band.style.left).toBe("25.00%");
    expect(band.style.width).toBe("25.00%");
  });

  it("clamps to the range and fills its parent without one", () => {
    const { getByRole, rerender } = render(
      <GapBand {...props} fromMs={-500} toMs={9_000} />
    );
    expect(getByRole("note").style.width).toBe("100.00%");
    rerender(<GapBand fromMs={1} toMs={2} label="Paused" />);
    expect(getByRole("note").style.left).toBe("");
  });

  it("hides the label when asked, but keeps the accessible name", () => {
    const { getByRole, queryByText } = render(
      <GapBand {...props} labelPosition="none" />
    );
    expect(queryByText(props.label)).toBeNull();
    expect(getByRole("note", { name: props.label })).toBeTruthy();
  });
});

describe("GapBands", () => {
  const range = { rangeFromMs: 0, rangeToMs: 4_000 };

  it("draws only the gaps that overlap the range", () => {
    const { getAllByRole } = render(
      <GapBands
        {...range}
        gaps={[
          { fromMs: -3_000, toMs: -1_000, label: "Kelvo not running" },
          { fromMs: 1_000, toMs: 2_000, label: "Asleep" },
          { fromMs: 4_000, toMs: 5_000, label: "Paused" },
        ]}
      />
    );
    expect(
      getAllByRole("note").map((n) => n.getAttribute("aria-label"))
    ).toEqual(["Asleep"]);
  });

  it("clips an open gap to the right edge", () => {
    const { getByRole } = render(
      <GapBands
        {...range}
        gaps={[
          { fromMs: 3_000, toMs: Number.MAX_SAFE_INTEGER, label: "Paused" },
        ]}
      />
    );
    const band = getByRole("note", { name: "Paused" });
    expect(band.style.left).toBe("75.00%");
    expect(band.style.width).toBe("25.00%");
  });

  it("keeps two gaps that start in the same millisecond apart", () => {
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    const asleep = { fromMs: 1_000, toMs: 2_000, label: "Asleep" };
    const paused = { fromMs: 1_000, toMs: 3_000, label: "Paused" };
    const { getAllByRole, rerender } = render(
      <GapBands {...range} gaps={[asleep, paused]} />
    );
    rerender(<GapBands {...range} gaps={[paused]} />);
    const names = getAllByRole("note").map((n) => n.getAttribute("aria-label"));
    expect(names).toEqual(["Paused"]);
    expect(errors).not.toHaveBeenCalled();
    errors.mockRestore();
  });
});
