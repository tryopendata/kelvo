import { createMockTransport } from "@core/mock-transport";
import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithProviders } from "../../../../../../tests/test-utils";
import { UpdateRow } from "./update-row";

describe("app updates", () => {
  it("checks manually even with automatic checks disabled", async () => {
    const transport = createMockTransport();
    await transport.updateSettings({ general: { check_updates: false } });
    const { user } = renderWithProviders(<UpdateRow />, { transport });
    await user.click(screen.getByRole("button", { name: "Check now" }));
    expect(
      await screen.findByText("You’re running the latest version.")
    ).toBeVisible();
    expect(
      transport.calls.some((call) => call.command === "check_for_updates")
    ).toBe(true);
    expect(
      transport.calls.some((call) => call.command === "install_update")
    ).toBe(false);
  });

  it("only installs the displayed version after the user clicks", async () => {
    const { transport, user } = renderWithProviders(<UpdateRow />, {
      transportOptions: {
        updateStatus: {
          kind: "available",
          version: "0.2.0",
          notes: "Fixes and improvements",
        },
      },
    });
    const button = await screen.findByRole("button", {
      name: "Install and restart",
    });
    expect(
      transport.calls.some((call) => call.command === "install_update")
    ).toBe(false);
    await user.click(button);
    expect(
      transport.calls.find((call) => call.command === "install_update")?.args
    ).toEqual(["0.2.0"]);
    expect(
      await screen.findByText("Update installed. Restarting Kelvo…")
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Please wait…" })).toBeDisabled();
  });

  it("surfaces signature failure and lets the user check again", async () => {
    const { user } = renderWithProviders(<UpdateRow />, {
      transportOptions: {
        updateStatus: { kind: "available", version: "0.2.0", notes: "" },
        updateFails: true,
      },
    });
    await user.click(
      await screen.findByRole("button", { name: "Install and restart" })
    );
    expect(
      await screen.findByText(/signature could not be verified/)
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "Check now" })).toBeEnabled();
  });

  it("recovers from rejected IPC without leaving controls stuck", async () => {
    const transport = createMockTransport();
    transport.checkForUpdates = () =>
      Promise.reject(new Error("Connection lost"));
    const { user } = renderWithProviders(<UpdateRow />, { transport });
    await user.click(screen.getByRole("button", { name: "Check now" }));
    await waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent("Connection lost")
    );
    expect(screen.getByRole("button", { name: "Check now" })).toBeEnabled();
  });
});
