import type { HistoryHealth } from "@core/generated/bindings";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useHistoryHealth } from "./use-history-health";

function Probe() {
  const { health, error } = useHistoryHealth();
  if (error) return <p>error {error.kind}</p>;
  if (!health) return <p>loading</p>;
  return <p>low disk {String(health.low_disk_paused)}</p>;
}

const HEALTHY: HistoryHealth = {
  low_disk_paused: false,
  trimmed_before_ms: null,
  trimmed_limit_bytes: null,
  cap_met: true,
};

const healthCalls = (calls: { command: string }[]) =>
  calls.filter((c) => c.command === "history_health").length;

describe("useHistoryHealth", () => {
  it("replaces the cached health with the event's, without a refetch", async () => {
    const { transport } = renderWithProviders(<Probe />);
    await screen.findByText("low disk false");
    act(() =>
      transport.setHistoryHealth({ ...HEALTHY, low_disk_paused: true })
    );
    await screen.findByText("low disk true");
    expect(healthCalls(transport.calls)).toBe(1);
  });

  it("keeps history_unavailable when a health event arrives while history is still down", async () => {
    const { transport } = renderWithProviders(<Probe />, {
      transportOptions: { scenarios: ["history-unavailable"] },
    });
    await screen.findByText("error history_unavailable");
    // A stray event (the store is still unavailable) must not turn the error
    // into a healthy value: the command decides, so the hook asks it again.
    act(() => transport.setHistoryHealth(HEALTHY));
    await waitFor(() => expect(healthCalls(transport.calls)).toBe(2));
    expect(screen.getByText("error history_unavailable")).toBeInTheDocument();
  });

  it("clears the error once a reset brings history back", async () => {
    const { transport } = renderWithProviders(<Probe />, {
      transportOptions: { scenarios: ["history-unavailable"] },
    });
    await screen.findByText("error history_unavailable");
    await act(async () => {
      await transport.resetHistory();
    });
    await screen.findByText("low disk false");
  });
});
