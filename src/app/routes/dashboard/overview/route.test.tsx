import { MOCK_HOST_ID } from "@core/mock/fixtures";
import { createMockTransport } from "@core/mock-transport";
import { screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import OverviewRoute from "./route";

const interest = (calls: { command: string; args: unknown[] }[]) =>
  calls
    .filter((c) => c.command === "set_process_interest")
    .map((c) => c.args[1]);

describe("Overview process interest", () => {
  it("asks for process rows while mounted and withdraws on unmount", async () => {
    const { transport, unmount } = renderWithProviders(<OverviewRoute />);
    await screen.findByRole("heading", { name: "Overview" });
    await waitFor(() => expect(interest(transport.calls)).toEqual([true]));
    unmount();
    await waitFor(() =>
      expect(interest(transport.calls)).toEqual([true, false])
    );
  });

  it("asks for network rates only when the host can attribute them (D-081)", async () => {
    const views = (calls: { command: string; args: unknown[] }[]) =>
      calls
        .filter((c) => c.command === "set_process_interest")
        .map((c) => c.args[2] as { sort: string[]; network?: boolean });
    const on = renderWithProviders(<OverviewRoute />);
    await waitFor(() =>
      expect(views(on.transport.calls).at(-1)).toMatchObject({
        sort: ["cpu", "memory", "energy", "disk_total", "net_total", "gpu"],
        network: true,
      })
    );
    on.unmount();

    const off = renderWithProviders(<OverviewRoute />, {
      transportOptions: { scenarios: ["no-process-network"] },
    });
    await waitFor(() => expect(views(off.transport.calls)).toHaveLength(1));
    expect(views(off.transport.calls)[0]?.network).toBeUndefined();
    expect(views(off.transport.calls)[0]?.sort).not.toContain("net_total");
  });

  it("asks for GPU time only when the host can attribute it (D-085)", async () => {
    const views = (calls: { command: string; args: unknown[] }[]) =>
      calls
        .filter((c) => c.command === "set_process_interest")
        .map((c) => c.args[2] as { sort: string[]; gpu?: boolean });
    const on = renderWithProviders(<OverviewRoute />, {
      transportOptions: { scenarios: ["no-process-network"] },
    });
    await waitFor(() =>
      expect(views(on.transport.calls).at(-1)).toMatchObject({
        sort: ["cpu", "memory", "energy", "disk_total", "gpu"],
        gpu: true,
      })
    );
    on.unmount();

    const off = renderWithProviders(<OverviewRoute />, {
      transportOptions: { scenarios: ["no-process-gpu"] },
    });
    await waitFor(() => expect(views(off.transport.calls)).toHaveLength(1));
    expect(views(off.transport.calls)[0]?.gpu).toBeUndefined();
    expect(views(off.transport.calls)[0]?.sort).not.toContain("gpu");
  });

  it("logs a failed set_process_interest with the host and window", async () => {
    const transport = createMockTransport({ autoTick: false });
    transport.setProcessInterest = async () => ({
      status: "error",
      error: { kind: "unknown_host", host: MOCK_HOST_ID },
    });
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    renderWithProviders(<OverviewRoute />, { transport });
    await waitFor(() =>
      expect(log).toHaveBeenCalledWith(
        "[processes] set_process_interest failed",
        expect.objectContaining({ hostId: MOCK_HOST_ID, window: "dashboard" })
      )
    );
    log.mockRestore();
  });
});
