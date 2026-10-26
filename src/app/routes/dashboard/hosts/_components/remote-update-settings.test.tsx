import { createMockTransport } from "@core/mock-transport";
import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../../../../../tests/test-utils";
import {
  COORDINATED_UPGRADE_GUIDE,
  RemoteUpdateSettings,
} from "./remote-update-settings";

describe("remote update settings", () => {
  it("offers a coordinated upgrade guide when another update request cannot help", async () => {
    const transport = createMockTransport({ scenarios: ["multi-host"] });
    transport.getRemoteUpdateStatus = async () => ({
      status: "ok",
      data: {
        supported: true,
        check_updates: true,
        auto_install_updates: true,
        status: "compatibility_blocked",
        version: "9.0.0",
        message: "This release needs a coordinated upgrade.",
      },
    });
    transport.openUrl = vi.fn(async () => {});
    const { user } = renderWithProviders(
      <RemoteUpdateSettings hostId="remote-studio" />,
      { transport }
    );
    await user.click(
      await screen.findByRole("button", { name: "Upgrade guide" })
    );
    expect(transport.openUrl).toHaveBeenCalledWith(COORDINATED_UPGRADE_GUIDE);
  });
  it("reads and changes the host policy independently of this Mac", async () => {
    const transport = createMockTransport({ scenarios: ["multi-host"] });
    await transport.updateSettings({
      general: { check_updates: false, auto_install_updates: false },
    });
    const { user, unmount } = renderWithProviders(
      <RemoteUpdateSettings hostId="remote-studio" />,
      { transport }
    );
    expect(
      await screen.findByRole("switch", {
        name: "Install host updates automatically",
      })
    ).toBeChecked();
    await user.click(
      screen.getByRole("switch", { name: "Install host updates automatically" })
    );
    await waitFor(() =>
      expect(
        screen.getByRole("switch", {
          name: "Install host updates automatically",
        })
      ).not.toBeChecked()
    );
    expect((await transport.getSettings()).settings.general.check_updates).toBe(
      false
    );
    unmount();
    renderWithProviders(<RemoteUpdateSettings hostId="remote-studio" />, {
      transport,
    });
    expect(
      await screen.findByRole("switch", {
        name: "Install host updates automatically",
      })
    ).not.toBeChecked();
    expect(
      screen.getByRole("switch", {
        name: "Check for host updates automatically",
      })
    ).toBeChecked();
  });

  it("disabling checks disables installation without forgetting its preference", async () => {
    const { user } = renderWithProviders(
      <RemoteUpdateSettings hostId="remote-studio" />,
      { transportOptions: { scenarios: ["multi-host"] } }
    );
    await user.click(
      await screen.findByRole("switch", {
        name: "Check for host updates automatically",
      })
    );
    await waitFor(() =>
      expect(
        screen.getByRole("switch", {
          name: "Install host updates automatically",
        })
      ).toBeDisabled()
    );
    expect(
      screen.getByRole("switch", { name: "Install host updates automatically" })
    ).toBeChecked();
  });

  it("explains the first upgrade for legacy agents instead of showing an enabled switch", async () => {
    renderWithProviders(<RemoteUpdateSettings hostId="remote-linux" />, {
      transportOptions: { scenarios: ["linux-host", "linux-agent-update"] },
    });
    expect(
      await screen.findByText(
        "Update this host once to enable automatic updates."
      )
    ).toBeVisible();
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
  });

  it("shows failed reads without inventing enabled settings and permits retry", async () => {
    const transport = createMockTransport({ scenarios: ["multi-host"] });
    const read = transport.getRemoteUpdateStatus;
    transport.getRemoteUpdateStatus = async () => {
      throw new Error("SSH connection lost");
    };
    const { user } = renderWithProviders(
      <RemoteUpdateSettings hostId="remote-studio" />,
      { transport }
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "SSH connection lost"
    );
    expect(screen.queryByRole("switch")).not.toBeInTheDocument();
    transport.getRemoteUpdateStatus = read;
    await user.click(screen.getByRole("button", { name: "Refresh" }));
    expect(
      await screen.findByRole("switch", {
        name: "Install host updates automatically",
      })
    ).toBeChecked();
  });
});
