import { fireEvent, render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { OVERVIEW_CARDS, PASSIVE_COOLING_CARD } from "./lib/sample-props";
import { MetricCard, type MetricCardProps } from "./metric-card";

const CPU = OVERVIEW_CARDS[0] as MetricCardProps;
const GPU = OVERVIEW_CARDS[1] as MetricCardProps;

describe("MetricCard", () => {
  it("round-trips its props through JSON for every sample card", () => {
    for (const card of OVERVIEW_CARDS) expectJsonRoundTrip(MetricCard, card);
    expectJsonRoundTrip(MetricCard, PASSIVE_COOLING_CARD);
  });

  it("is a link named by its title that hands navigation to the route", () => {
    const onOpen = vi.fn();
    render(<MetricCard {...CPU} onOpen={onOpen} />);
    fireEvent.click(screen.getByRole("link", { name: "CPU" }));
    expect(onOpen).toHaveBeenCalledWith("/dashboard/cpu");
  });

  it("is a plain region without an href", () => {
    const { href: _href, ...rest } = CPU;
    render(<MetricCard {...rest} />);
    expect(screen.getByRole("region", { name: "CPU" })).toBeInTheDocument();
  });

  it("renders the GPU chart body instead of a list", () => {
    render(<MetricCard {...GPU} />);
    expect(
      screen.getByRole("img", { name: "GPU, last 60 seconds, 36%" })
    ).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: /processes/ })).toBeNull();
  });
});
