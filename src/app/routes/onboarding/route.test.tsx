import { act, screen, waitFor, within } from "@testing-library/react";
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

  it("is one tab stop, and the arrow keys move the selection", async () => {
    const { user } = renderWithProviders(<OnboardingRoute />);
    const group = await screen.findByRole("radiogroup", {
      name: "Menu bar style",
    });
    const cards = within(group).getAllByRole("radio");
    const [combined, graphs] = cards;
    expect(combined).toHaveAttribute("aria-checked", "true");
    act(() => combined?.focus());
    expect(cards.map((r) => r.tabIndex)).toEqual([0, -1, -1]);
    // Held down: Radix moves focus on a timer and checks the radio it lands
    // on only while the arrow key is still pressed, as in a browser.
    await user.keyboard("{ArrowDown>}");
    await waitFor(() => expect(graphs).toHaveAttribute("aria-checked", "true"));
    await user.keyboard("{/ArrowDown}");
    expect(graphs).toHaveFocus();
    expect(combined).toHaveAttribute("aria-checked", "false");
    expect(cards.map((r) => r.tabIndex)).toEqual([-1, 0, -1]);
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
      const { menu_bar } = (await transport.getSettings()).settings;
      expect(menu_bar?.items).toMatchObject({
        cpu: "graph",
        memory: "graph",
        network: "graph",
        gpu: "off",
        power: "off",
      });
      expect(menu_bar?.bars).toEqual({ cpu: false, gpu: false, memory: false });
      expect(menu_bar?.readouts.temperature).toBe(false);
    });
  });
});
