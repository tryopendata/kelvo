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

/** Reports what a card would read from `useChartWindow`. */
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

const radio = (name: string) => screen.getByRole("radio", { name });
const settingsWrites = (calls: { command: string; args: unknown[] }[]) =>
  calls.filter((c) => c.command === "update_settings").map((c) => c.args[0]);

beforeEach(() => {
  seen.length = 0;
});

describe("WindowControl and useChartWindow (D-091)", () => {
  it("is null until settings load, then shows the saved 15m", async () => {
    renderControl();
    expect(seen[0]).toBeNull();
    expect(screen.queryByRole("radio")).toBeNull();
    expect(await screen.findByRole("radio", { name: "15m" })).toHaveAttribute(
      "aria-checked",
      "true"
    );
    expect(screen.getByLabelText("window ms")).toHaveTextContent("900000");
    expect(screen.queryByRole("radio", { name: "1m" })).toBeNull();
  });

  it("saves a pick once and every reader follows", async () => {
    const { transport, user } = renderControl();
    await user.click(await screen.findByRole("radio", { name: "30m" }));
    await waitFor(() =>
      expect(radio("30m")).toHaveAttribute("aria-checked", "true")
    );
    expect(screen.getByLabelText("window ms")).toHaveTextContent("1800000");
    expect(settingsWrites(transport.calls)).toEqual([
      { general: { chart_window: "30m" } },
    ]);
  });

  it("follows a change saved by another window", async () => {
    const { transport } = renderControl();
    await screen.findByRole("radio", { name: "15m" });
    await transport.updateSettings({ general: { chart_window: "1h" } });
    await waitFor(() =>
      expect(radio("1h")).toHaveAttribute("aria-checked", "true")
    );
    expect(screen.getByLabelText("window ms")).toHaveTextContent("3600000");
  });

  it("at 60 s sampling disables 5m and shows 15m without overwriting the saved 5m", async () => {
    const { transport } = renderControl({
      intervalMs: 60_000,
      chartWindow: "5m",
    });
    await waitFor(() =>
      expect(screen.getByRole("radio", { name: "5m" })).toBeDisabled()
    );
    await waitFor(() =>
      expect(radio("15m")).toHaveAttribute("aria-checked", "true")
    );
    expect(screen.getByLabelText("window ms")).toHaveTextContent("900000");
    expect(settingsWrites(transport.calls)).toEqual([]);
    expect((await transport.getSettings()).settings.general.chart_window).toBe(
      "5m"
    );
  });

  it("keeps the selection and says why when the save fails", async () => {
    const { transport, user } = renderControl({ settingsNotSaved: true });
    await user.click(await screen.findByRole("radio", { name: "30m" }));
    expect(
      await screen.findByText("Couldn't save settings. Nothing changed.")
    ).toBeInTheDocument();
    expect(radio("15m")).toHaveAttribute("aria-checked", "true");
    expect(settingsWrites(transport.calls)).toHaveLength(1);
  });
});
