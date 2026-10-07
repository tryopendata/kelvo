import { createMockTransport } from "@core/mock-transport";
import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Route, Routes } from "react-router";
import DiskRoute from "../disk/route";
import GpuRoute from "../gpu/route";
import CpuRoute from "./route";

function Pages() {
  return (
    <Routes>
      <Route path="cpu" element={<CpuRoute />} />
      <Route path="gpu" element={<GpuRoute />} />
      <Route path="disk" element={<DiskRoute />} />
    </Routes>
  );
}

describe("CPU page chart window (D-091)", () => {
  it("applies a window picked on CPU to its charts and to the GPU and Disk pages", async () => {
    const { router, transport, user } = renderWithProviders(<Pages />, {
      route: "/cpu",
    });
    await user.click(await screen.findByRole("radio", { name: "30m" }));
    expect(
      await screen.findByRole("img", {
        name: /^CPU total and system, last 30 minutes,/,
      })
    ).toBeVisible();
    expect(
      screen.getByRole("heading", { name: "Per-core load, last 30 minutes" })
    ).toBeVisible();
    expect(
      transport.calls
        .filter((c) => c.command === "update_settings")
        .map((c) => c.args[0])
    ).toEqual([{ general: { chart_window: "30m" } }]);

    await act(() => router.navigate("/gpu"));
    expect(
      await screen.findByRole("heading", { name: "Power, last 30 minutes" })
    ).toBeVisible();
    expect(screen.getByRole("radio", { name: "30m" })).toBeChecked();
    expect(
      screen.getByRole("img", { name: /^GPU power, last 30 minutes,/ })
    ).toBeVisible();

    await act(() => router.navigate("/disk"));
    expect(
      await screen.findByRole("img", { name: /last 30 minutes/ })
    ).toBeVisible();
    expect(screen.getByRole("radio", { name: "30m" })).toBeChecked();
  });

  it("draws at the default 15m when settings could not be read", async () => {
    const transport = createMockTransport({ autoTick: false });
    transport.getSettings = () => Promise.reject(new Error("bridge down"));
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    onTestFinished(() => error.mockRestore());
    renderWithProviders(<CpuRoute />, { transport });
    expect(
      await screen.findByRole("img", {
        name: /^CPU total and system, last 15 minutes,/,
      })
    ).toBeVisible();
    expect(screen.getByRole("radio", { name: "15m" })).toBeChecked();
    expect(error).toHaveBeenCalledWith(
      "[settings] get_settings failed",
      expect.anything()
    );
  });
});
