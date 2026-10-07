import { render } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { StreamArea, type StreamAreaProps } from "./stream-area";

const props: StreamAreaProps = {
  series: [
    { key: "total", values: [10, 12, 14, 13, 18], step: 1 },
    { key: "system", values: [3, 4, 4, 5, 5.6], step: 2 },
  ],
  tEndMs: 1_760_000_000_000,
  intervalMs: 1000,
  yMax: 40,
  accent: "cpu",
  height: 52,
  ariaLabel: "CPU, last 60 seconds",
  ceilingLabel: "40%",
  windowLabel: "60s",
  gridlines: [20],
};

function moveCount(d: string | null | undefined): number {
  return (d ?? "").match(/M/g)?.length ?? 0;
}

describe("StreamArea", () => {
  it("renders the same after a JSON round trip", () => {
    expectJsonRoundTrip(StreamArea, props);
  });

  it("places gap bands on its own time axis", () => {
    // Five values 1 s apart end at tEndMs: the axis is the last 4 s.
    const { getByRole, rerender, queryByRole } = render(
      <StreamArea
        {...props}
        gaps={[
          {
            fromMs: props.tEndMs - 3_000,
            toMs: props.tEndMs - 2_000,
            label: "Asleep 11:02–11:31 · not interpolated",
          },
        ]}
      />
    );
    const band = getByRole("note", {
      name: "Asleep 11:02–11:31 · not interpolated",
    });
    expect(band.style.left).toBe("25.00%");
    expect(band.style.width).toBe("25.00%");
    rerender(
      <StreamArea
        {...props}
        gaps={[{ fromMs: 0, toMs: props.tEndMs - 10_000, label: "Paused" }]}
      />
    );
    expect(queryByRole("note")).toBeNull();
  });

  it("is an img named by its aria-label", () => {
    const { getByRole } = render(<StreamArea {...props} />);
    expect(getByRole("img", { name: "CPU, last 60 seconds" })).toBeTruthy();
  });

  it("breaks the line at a null instead of bridging it", () => {
    const { container } = render(
      <StreamArea
        {...props}
        series={[{ key: "total", values: [10, 12, null, 13, 18], step: 1 }]}
      />
    );
    const d = container.querySelector("[data-line]")?.getAttribute("d");
    expect(moveCount(d)).toBe(2);
    // A hollow dot on each edge that borders the gap.
    expect(container.querySelectorAll("[data-gap-edge]")).toHaveLength(2);
  });

  it("draws one segment and no edge dots when nothing is missing", () => {
    const { container } = render(<StreamArea {...props} />);
    const d = container.querySelector("[data-line]")?.getAttribute("d");
    expect(moveCount(d)).toBe(1);
    expect(container.querySelectorAll("[data-gap-edge]")).toHaveLength(0);
  });

  it("leaves no stale edge dots when isolated points fill back in", () => {
    // Every other slot missing: each value is a one-point run with gaps on
    // both sides. The next tick has no gaps, so no dot may remain.
    const { container, rerender } = render(
      <StreamArea
        {...props}
        series={[
          { key: "total", values: [10, null, 12, null, 14, null], step: 1 },
        ]}
      />
    );
    expect(container.querySelectorAll("[data-gap-edge]")).toHaveLength(3);
    rerender(
      <StreamArea
        {...props}
        series={[{ key: "total", values: [10, 11, 12, 13, 14, 15], step: 1 }]}
      />
    );
    expect(container.querySelectorAll("[data-gap-edge]")).toHaveLength(0);
  });
});
