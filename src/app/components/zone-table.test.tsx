import { render, screen } from "@testing-library/react";
import { type ZoneRow, ZoneTable } from "./zone-table";

const zones = (n: number): ZoneRow[] =>
  Array.from({ length: n }, (_, i) => ({
    key: `PMU tdie${i}`,
    name: `Zone ${String(i + 1).padStart(2, "0")}`,
    now: 50,
    min: 40,
    max: 60,
  }));

describe("ZoneTable (D-093)", () => {
  it("scrolls past ten and a half rows under a pinned header, reachable by keyboard", () => {
    render(
      <ZoneTable rows={zones(20)} extras={[]} units="C" rangeLabel="15m" />
    );
    const scroll = screen.getByRole("region", { name: "SoC thermal zones" });
    // 22 px header + 10.5 rows of 24 px.
    expect(scroll).toHaveStyle({ maxHeight: "274px" });
    expect(scroll).toHaveAttribute("tabindex", "0");
    expect(screen.getAllByRole("rowgroup")[0]).toHaveClass("sticky");
    expect(screen.getAllByRole("row")).toHaveLength(21);
  });

  it("takes no tab stop when every zone fits", () => {
    render(
      <ZoneTable rows={zones(10)} extras={[]} units="C" rangeLabel="15m" />
    );
    expect(screen.queryByRole("region")).toBeNull();
    expect(screen.getByTestId("zone-scroll")).not.toHaveAttribute("tabindex");
  });
});
