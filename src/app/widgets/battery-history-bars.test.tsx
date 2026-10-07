import { render } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import {
  BatteryHistoryBars,
  type BatteryHistoryBarsProps,
} from "./battery-history-bars";

const H = 3_600_000;
const T0 = 1_760_000_000_000 - (1_760_000_000_000 % H);

const props: BatteryHistoryBarsProps = {
  hours: [68, 64, null, 66, 74, 80].map((charge, i) => ({
    tsMs: T0 + i * H,
    charge,
    charging: i >= 3,
  })),
  annotations: [{ tsMs: T0 + H, label: "Optimized charging: held at 80%" }],
};

describe("BatteryHistoryBars", () => {
  it("renders the same after a JSON round trip", () => {
    expectJsonRoundTrip(BatteryHistoryBars, props);
  });

  it("names the chart with the hourly values", () => {
    const { getByRole } = render(<BatteryHistoryBars {...props} />);
    expect(getByRole("img").getAttribute("aria-label")).toMatch(/68%/);
  });

  it("hatches an hour with no samples", () => {
    const { container } = render(<BatteryHistoryBars {...props} />);
    const gaps = container.querySelectorAll<HTMLElement>("[data-gap]");
    expect(gaps).toHaveLength(1);
    expect(gaps[0]?.title).toMatch(/no samples/);
  });

  it("marks charging hours and labels every third hour", () => {
    const { container } = render(<BatteryHistoryBars {...props} />);
    const marks = [...container.querySelectorAll<HTMLElement>(".h-1")].filter(
      (m) => m.style.background === "var(--a)"
    );
    expect(marks).toHaveLength(3);
    const labels = [...container.querySelectorAll(".text-center")]
      .map((l) => l.textContent)
      .filter(Boolean);
    expect(labels).toHaveLength(2);
  });

  it("shows the annotation as a pill", () => {
    const { getByText } = render(<BatteryHistoryBars {...props} />);
    expect(getByText("Optimized charging: held at 80%").className).toMatch(
      /border/
    );
  });
});
