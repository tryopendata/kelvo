import { render, screen } from "@testing-library/react";
import { expectJsonRoundTrip } from "@tests/widget-json";
import { StatusPill } from "./status-pill";

describe("StatusPill", () => {
  it("round-trips its props through JSON in every state", () => {
    expectJsonRoundTrip(StatusPill, { state: "live", label: "1s" });
    expectJsonRoundTrip(StatusPill, { state: "paused", label: "Paused" });
    expectJsonRoundTrip(StatusPill, { state: "stale", label: "Stale" });
  });

  it("always carries the state word", () => {
    render(<StatusPill state="paused" label="Paused" />);
    expect(screen.getByText("Paused")).toBeInTheDocument();
  });
});
