import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { LiveSidebar } from "~/routes/dashboard/_components/live-sidebar";
import PopoverRoute from "~/routes/popover/route";

const LOW_POWER_WHY =
  /turned on because macOS Low Power Mode is on\. It turns off when Low Power Mode does\./;

describe("Performance mode markers (D-088)", () => {
  it("marks the popover pill, explains it on focus, and clears with Low Power Mode", async () => {
    const { transport, user } = renderWithProviders(<PopoverRoute />, {
      transportOptions: { scenarios: ["low-power-mode"] },
    });
    await screen.findByText("Open dashboard");
    act(() => transport.tick());
    const marker = await screen.findByRole("button", {
      name: "2s · perf, open Performance settings",
    });
    expect(marker).toHaveAccessibleDescription(LOW_POWER_WHY);
    expect(marker).toHaveAccessibleDescription(/Animations off/);

    act(() => marker.focus());
    const link = await screen.findByRole("button", {
      name: "Performance settings",
    });
    await user.click(link);
    expect(transport.calls).toContainEqual({
      command: "open_dashboard",
      args: ["/dashboard/settings"],
    });

    act(() => transport.setLowPowerMode(false));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: /perf/ })).toBeNull()
    );
    expect(screen.getByText("1s")).toBeInTheDocument();
  });

  it("adds a sidebar line naming the reason", async () => {
    const { transport } = renderWithProviders(<LiveSidebar />, {
      transportOptions: { scenarios: ["low-power-mode"] },
    });
    expect(
      await screen.findByRole("button", {
        name: "Performance mode · Low Power, open Performance settings",
      })
    ).toHaveAccessibleDescription(LOW_POWER_WHY);

    await act(() =>
      transport.updateSettings({ sampling: { performance_mode: true } })
    );
    const marker = await screen.findByRole("button", {
      name: "Performance mode, open Performance settings",
    });
    expect(marker).toHaveAccessibleDescription(
      /You turned on Performance mode in Settings\./
    );

    act(() => transport.setLowPowerMode(false));
    await act(() =>
      transport.updateSettings({ sampling: { performance_mode: false } })
    );
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /Performance mode/ })
      ).toBeNull()
    );
  });
});
