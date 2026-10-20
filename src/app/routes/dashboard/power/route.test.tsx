import type { ScenarioName } from "@core/mock/fixtures";
import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import PowerRoute from "./route";

const NOW = 1_800_000_000_000;

describe("Power cards follow the chart window (D-091)", () => {
  it("titles the power stack and heads the zone range with the saved window", async () => {
    const r = renderWithProviders(<PowerRoute />, {
      transportOptions: { now: () => NOW, chartWindow: "30m" },
    });
    expect(
      await screen.findByText("Power by component, last 30 minutes")
    ).toBeInTheDocument();
    act(() => r.transport.tick());
    expect(
      await screen.findByRole("columnheader", { name: "30m" })
    ).toBeInTheDocument();
  });
});

describe("Power page at 1 Hz", () => {
  // The page body (window control, cards, Energy by app) re-renders only when
  // the page's own subscriptions change; each card subscribes to its values.
  // Calling the route as a function runs its hooks in the spy, so the spy
  // renders exactly when the page does.
  it.each<[string, ScenarioName[], string]>([
    ["a Mac", ["default"], "Power by component, last hour"],
    ["a Linux host", ["linux"], "System power, last hour"],
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
