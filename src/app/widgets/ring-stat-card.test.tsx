import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { PASSIVE_FANS_CARD, RING_STAT_CARDS } from "./lib/sample-props";
import { RingStatCard } from "./ring-stat-card";

describe("RingStatCard", () => {
  it("round-trips its props through JSON", () => {
    for (const card of RING_STAT_CARDS) expectJsonRoundTrip(RingStatCard, card);
    expectJsonRoundTrip(RingStatCard, PASSIVE_FANS_CARD);
  });

  it("drops the ring for passive cooling", () => {
    const { container } = render(<RingStatCard {...PASSIVE_FANS_CARD} />);
    expect(screen.getByRole("region", { name: "Fans" })).toBeInTheDocument();
    expect(container.querySelector("svg")).toBeNull();
  });
});
