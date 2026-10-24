import { dateInput } from "@core/chart-range";
import type { MockTransportOptions } from "@core/mock-transport";
import { screen, waitFor } from "@testing-library/react";
import { Toaster } from "~/components/ui/sonner";
import {
  type ChartWindowState,
  useChartWindow,
} from "~/hooks/use-chart-window";
import { renderWithProviders } from "../../../tests/test-utils";
import { WindowControl } from "./window-control";

const seen: (ChartWindowState | null)[] = [];
function Probe() {
  const w = useChartWindow();
  seen.push(w);
  return <output aria-label="window ms">{w?.windowMs ?? "loading"}</output>;
}
function renderControl(transportOptions?: MockTransportOptions) {
  return renderWithProviders(
    <>
      <WindowControl />
      <Probe />
      <Toaster />
    </>,
    { transportOptions }
  );
}
const settingsWrites = (calls: { command: string; args: unknown[] }[]) =>
  calls.filter((c) => c.command === "update_settings").map((c) => c.args[0]);
beforeEach(() => {
  seen.length = 0;
});

describe("WindowControl", () => {
  it("waits for settings, then shows two shortcuts and the saved range in the dropdown", async () => {
    renderControl();
    expect(seen[0]).toBeNull();
    expect(
      await screen.findByRole("button", { name: "Time range: 15m" })
    ).toBeVisible();
    expect(screen.getAllByRole("radio").map((r) => r.textContent)).toEqual([
      "5m",
      "1h",
    ]);
    expect(screen.getByLabelText("window ms")).toHaveTextContent("900000");
  });
  it("applies a long preset once, closes the popup and shows the active duration", async () => {
    const { user, transport } = renderControl();
    await user.click(
      await screen.findByRole("button", { name: "Time range: 15m" })
    );
    await user.click(screen.getByRole("button", { name: "Last 6 hours" }));
    await screen.findByRole("button", { name: "Time range: 6h" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByLabelText("window ms")).toHaveTextContent("21600000");
    expect(settingsWrites(transport.calls)).toEqual([
      { general: { chart_window: { relative: 21_600_000 } } },
    ]);
  });
  it("applies a custom rolling duration and keeps its exact length", async () => {
    const { user, transport } = renderControl();
    await user.click(
      await screen.findByRole("button", { name: "Time range: 15m" })
    );
    await user.clear(screen.getByLabelText("Duration"));
    await user.type(screen.getByLabelText("Duration"), "90");
    await user.click(screen.getByRole("button", { name: "Apply" }));
    await screen.findByRole("button", { name: "Time range: 90m" });
    expect(settingsWrites(transport.calls)).toEqual([
      { general: { chart_window: { relative: 5_400_000 } } },
    ]);
  });
  it("keeps invalid dates in the form, then applies valid fixed dates", async () => {
    const { user, transport } = renderControl();
    await user.click(
      await screen.findByRole("button", { name: "Time range: 15m" })
    );
    await user.click(screen.getByRole("radio", { name: "Date range" }));
    await user.clear(screen.getByLabelText("From"));
    await user.type(screen.getByLabelText("From"), "2026-02-30 12:00");
    await user.click(screen.getByRole("button", { name: "Apply" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("valid dates");
    expect(settingsWrites(transport.calls)).toHaveLength(0);
    const to = Math.floor((Date.now() - 86_400_000) / 60_000) * 60_000;
    await user.clear(screen.getByLabelText("From"));
    await user.type(screen.getByLabelText("From"), dateInput(to - 3_600_000));
    await user.clear(screen.getByLabelText("To"));
    await user.type(screen.getByLabelText("To"), dateInput(to));
    await user.click(screen.getByRole("button", { name: "Apply" }));
    await screen.findByRole("button", { name: "Time range: Custom" });
    expect(settingsWrites(transport.calls)).toEqual([
      {
        general: {
          chart_window: { absolute: { from_ms: to - 3_600_000, to_ms: to } },
        },
      },
    ]);
  });
  it("follows settings saved by another window and returns to a live shortcut", async () => {
    const { user, transport } = renderControl();
    await screen.findByRole("button", { name: "Time range: 15m" });
    await transport.updateSettings({
      general: { chart_window: { relative: 604_800_000 } },
    });
    await screen.findByRole("button", { name: "Time range: 7d" });
    await user.click(screen.getByRole("radio", { name: "1h" }));
    await waitFor(() =>
      expect(screen.getByRole("radio", { name: "1h" })).toBeChecked()
    );
    expect(seen.at(-1)?.endMs).toBeNull();
  });
  it("disables an undersampled shortcut without changing the saved setting", async () => {
    const { transport } = renderControl({
      intervalMs: 60_000,
      chartWindow: "5m",
    });
    await screen.findByRole("button", { name: "Time range: 15m" });
    expect(screen.getByRole("radio", { name: "5m" })).toBeDisabled();
    expect(settingsWrites(transport.calls)).toEqual([]);
  });
  it("keeps the previous range when saving fails", async () => {
    const { user, transport } = renderControl({ settingsNotSaved: true });
    await user.click(
      await screen.findByRole("button", { name: "Time range: 15m" })
    );
    await user.click(screen.getByRole("button", { name: "Last 6 hours" }));
    await screen.findByText("Couldn't save settings. Nothing changed.");
    expect(
      screen.getByRole("button", { name: "Time range: 15m" })
    ).toBeVisible();
    expect(settingsWrites(transport.calls)).toHaveLength(1);
  });
  it("dismisses with Escape without applying a draft", async () => {
    const { user, transport } = renderControl();
    await user.click(
      await screen.findByRole("button", { name: "Time range: 15m" })
    );
    await user.type(screen.getByLabelText("Duration"), "9");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Time range: 15m" })
    ).toHaveFocus();
    expect(settingsWrites(transport.calls)).toHaveLength(0);
  });
});
