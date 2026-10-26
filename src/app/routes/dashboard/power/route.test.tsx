import type { ScenarioName } from "@core/mock/fixtures";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import PowerRoute from "./route";

const NOW = 1_800_000_000_000;

describe("Power cards follow the chart window (D-091)", () => {
  it("titles the power draw chart and the zone matrix with the saved window", async () => {
    const r = renderWithProviders(<PowerRoute />, {
      transportOptions: { now: () => NOW, chartWindow: "30m" },
    });
    expect(
      await screen.findByText("Power draw, last 30 minutes")
    ).toBeInTheDocument();
    act(() => r.transport.tick());
    expect(
      await screen.findByText("SoC thermal zones, last 30 minutes")
    ).toBeInTheDocument();
  });
});

describe("Power page at 1 Hz", () => {
  // The page body (window control, cards, Energy by app) re-renders only when
  // the page's own subscriptions change; each card subscribes to its values.
  // Calling the route as a function runs its hooks in the spy, so the spy
  // renders exactly when the page does.
  it.each<[string, ScenarioName[], string]>([
    ["a Mac", ["default"], "SoC thermal zones, last hour"],
    ["a Linux host", ["linux"], "Temperature sensors, last hour"],
  ])(
    "does not re-render the page on a tick on %s",
    async (_, scenarios, title) => {
      const renders = { n: 0 };
      function Spy() {
        renders.n++;
        return PowerRoute();
      }
      const r = renderWithProviders(<Spy />, {
        transportOptions: { now: () => NOW, chartWindow: "1h", scenarios },
      });
      expect(await screen.findByText(title)).toBeInTheDocument();
      act(() => r.transport.tick());
      const settled = renders.n;
      for (let i = 0; i < 5; i++) act(() => r.transport.tick());
      expect(renders.n).toBe(settled);
    }
  );
});

it("Battery enablement also controls Power's battery history without removing system draw", async () => {
  const r = renderWithProviders(<PowerRoute />, {
    transportOptions: { now: () => NOW },
  });
  await screen.findByText("Battery charge");
  await act(async () => {
    await r.transport.updateSettings({
      modules: { battery: { enabled: false } },
    });
  });
  await waitFor(() => expect(screen.queryByText("Battery charge")).toBeNull());
  expect(screen.getAllByText(/system draw/).length).toBeGreaterThan(0);
  await act(async () => {
    await r.transport.updateSettings({
      modules: { battery: { enabled: true } },
    });
  });
  await screen.findByText("Battery charge");
});
