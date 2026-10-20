import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { ZonesCard } from "./zones-card";

const NOW = 1_800_000_000_000;

describe("ZonesCard", () => {
  it("names each zone's sensor and current reading in the matrix label", async () => {
    const r = renderWithProviders(<ZonesCard windowMs={900_000} />, {
      transportOptions: { now: () => NOW },
    });
    await screen.findByText("SoC thermal zones, last 15 minutes");
    act(() => r.transport.tick());
    const matrix = await screen.findByRole("img", {
      name: /^SoC thermal zones/,
    });
    const label = matrix.getAttribute("aria-label") ?? "";
    expect(label).toMatch(/Zone 01 \(PMU tdie4\) \d+°/);
    expect(label).toMatch(/Zone 10 \(PMU2 tdie3\) \d+°/);
  });
});
