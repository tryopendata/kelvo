import { act, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "../../../../tests/test-utils";
import DashboardLayout from "./route";

function renderShell(
  scenarios: Parameters<typeof renderWithProviders>[1] = {}
) {
  return renderWithProviders(<DashboardLayout />, {
    route: "/dashboard/overview",
    ...scenarios,
  });
}

const sidebarLinks = () =>
  within(screen.getByRole("navigation"))
    .getAllByRole("link")
    .map((a) => a.textContent ?? "");

describe("Dashboard shell", () => {
  it("follows navigate-requested to a dashboard route", async () => {
    const { transport, router } = renderShell();
    act(() => transport.requestNavigate("/dashboard/settings"));
    expect(router.state.location.pathname).toBe("/dashboard/settings");
  });

  it("ignores navigate-requested for a route outside the dashboard", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { transport, router } = renderShell();
    act(() => transport.requestNavigate("/popover"));
    expect(router.state.location.pathname).toBe("/dashboard/overview");
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it("lists every module on a supported MacBook", async () => {
    renderShell();
    await waitFor(() =>
      expect(sidebarLinks().join("|")).toMatch(/Power & Sensors.*Battery/)
    );
  });

  it("leaves Battery out on a Mac without one", async () => {
    renderShell({ transportOptions: { scenarios: ["no-battery"] } });
    // Every module reads "on" until capabilities arrive, then Battery goes.
    await waitFor(() =>
      expect(sidebarLinks().some((t) => t.startsWith("Battery"))).toBe(false)
    );
    expect(sidebarLinks().some((t) => t.startsWith("Disk"))).toBe(true);
  });

  it("leaves Power & Sensors out on an unknown chip", async () => {
    renderShell({ transportOptions: { scenarios: ["unknown-chip"] } });
    await waitFor(() =>
      expect(sidebarLinks().some((t) => t.startsWith("Power"))).toBe(false)
    );
    expect(sidebarLinks().some((t) => t.startsWith("Battery"))).toBe(true);
  });
});
