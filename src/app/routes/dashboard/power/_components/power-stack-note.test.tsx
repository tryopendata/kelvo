import type { ScenarioName } from "@core/mock/fixtures";
import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { PowerStackCard } from "./power-stack-card";

const NOW = 1_800_000_000_000;

async function renderCard(scenarios: ScenarioName[]) {
  const r = renderWithProviders(<PowerStackCard windowMs={900_000} />, {
    transportOptions: { now: () => NOW, scenarios },
  });
  await screen.findByText("Power by component, last 15 minutes");
  act(() => r.transport.tick());
  return r;
}

describe("PowerStackCard CPU power note (D-054, D-065)", () => {
  it("says the CPU figure is P cores, uncalibrated, for source 1", async () => {
    await renderCard(["cpu-power-uncalibrated"]);
    expect(
      screen.getByText("CPU power: P cores, uncalibrated")
    ).toBeInTheDocument();
  });

  it("says the CPU figure is a carried-over estimate for source 3", async () => {
    await renderCard(["cpu-power-seeded"]);
    expect(
      screen.getByText("CPU power: P cores, estimated from last calibration")
    ).toBeInTheDocument();
  });

  it("still says P cores once calibrated (source 2)", async () => {
    await renderCard(["cpu-power-calibrated"]);
    expect(screen.getByText("CPU power: P cores")).toBeInTheDocument();
  });

  it("says nothing when CPU power is measured directly", async () => {
    await renderCard([]);
    expect(screen.queryByText(/^CPU power/)).toBeNull();
  });
});
