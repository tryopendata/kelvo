import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import PowerRoute from "./route";

const NOW = 1_800_000_000_000;

describe("Power cards follow the chart window (D-091)", () => {
  it("titles the power stack and the zone matrix with the saved window", async () => {
    const r = renderWithProviders(<PowerRoute />, {
      transportOptions: { now: () => NOW, chartWindow: "30m" },
    });
    expect(
      await screen.findByText("Power by component, last 30 minutes")
    ).toBeInTheDocument();
    act(() => r.transport.tick());
    expect(
      await screen.findByText("SoC thermal zones, last 30 minutes")
    ).toBeInTheDocument();
  });
});
