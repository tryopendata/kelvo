import type { ScenarioName } from "@core/mock/fixtures";
import { screen, waitFor, within } from "@testing-library/react";
import { Toaster } from "~/components/ui/sonner";
import { renderWithProviders } from "../../../../../tests/test-utils";
import SettingsRoute from "./route";

function renderSettings(scenarios: ScenarioName[]) {
  return renderWithProviders(
    <>
      <SettingsRoute />
      <Toaster />
    </>,
    { transportOptions: { scenarios } }
  );
}

describe("Settings: history unavailable and Reset history (D-064)", () => {
  it("says the file is damaged, confirms, resets and clears the banner", async () => {
    const { transport, user } = renderSettings(["history-corrupt"]);
    const banner = await screen.findByText(/The history file is damaged/);
    await user.click(
      within(banner.closest("[role=alert]") as HTMLElement).getByRole(
        "button",
        { name: "Reset history" }
      )
    );

    const dialog = await screen.findByRole("alertdialog", {
      name: "Reset history?",
    });
    expect(dialog).toHaveTextContent(/kept aside/);
    expect(dialog).toHaveTextContent(/not deleted/);
    await user.click(
      within(dialog).getByRole("button", { name: "Reset history" })
    );

    await waitFor(() =>
      expect(screen.queryByText(/The history file is damaged/)).toBeNull()
    );
    expect(transport.calls.map((c) => c.command)).toContain("reset_history");
    expect(
      await screen.findByText("History reset. The old file was kept aside.")
    ).toBeInTheDocument();
  });

  it("cancelling the confirm resets nothing", async () => {
    const { transport, user } = renderSettings(["history-corrupt"]);
    await user.click(
      await screen.findByRole("button", { name: "Reset history" })
    );
    const dialog = await screen.findByRole("alertdialog");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(transport.calls.map((c) => c.command)).not.toContain(
      "reset_history"
    );
    expect(screen.getByText(/The history file is damaged/)).toBeInTheDocument();
  });

  it("names a newer Kelvo's format and offers the reset", async () => {
    renderSettings(["history-too-new"]);
    expect(
      await screen.findByText(
        /written by a newer version of Kelvo \(format 4; this version reads up to 2\)/
      )
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Reset history" })
    ).toBeInTheDocument();
  });

  it("does not offer a reset while another Kelvo holds the file", async () => {
    renderSettings(["history-locked"]);
    expect(
      await screen.findByText(/another copy of Kelvo has the history file open/)
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Reset history" })).toBeNull();
  });
});
