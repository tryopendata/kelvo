import { screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "../../../../tests/test-utils";
import OnboardingRoute from "./route";

describe("Onboarding menu bar style", () => {
  it("offers the three menu bar style cards, Graph per module second", async () => {
    renderWithProviders(<OnboardingRoute />);
    const group = await screen.findByRole("radiogroup", {
      name: "Menu bar style",
    });
    const cards = within(group).getAllByRole("radio");
    expect(cards).toHaveLength(3);
    expect(cards[0]).toHaveTextContent(/^Combined/);
    expect(cards[1]).toHaveTextContent(/^Graph per module/);
    expect(cards[2]).toHaveTextContent(/^Values only/);
    expect(
      screen.getByText("Separate items you can reorder with ⌘-drag.")
    ).toBeInTheDocument();
  });

  it("Graph per module gives CPU, memory and network their own graph items", async () => {
    const { transport, user } = renderWithProviders(<OnboardingRoute />);
    const card = await screen.findByRole("radio", {
      name: /Graph per module/,
    });
    await user.click(card);
    expect(card).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("button", { name: "Continue" }));

    await waitFor(async () => {
      const { modules } = (await transport.getSettings()).settings;
      expect(modules.cpu?.menu_bar).toBe("own_graph");
      expect(modules.memory?.menu_bar).toBe("own_graph");
      expect(modules.network?.menu_bar).toBe("own_graph");
      expect(modules.gpu?.menu_bar).toBe("hidden");
      expect(modules.power?.menu_bar).toBe("hidden");
    });
  });
});
