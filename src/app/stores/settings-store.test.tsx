import { MOCK_HOST_ID } from "@core/mock/fixtures";
import { createMockTransport } from "@core/mock-transport";
import { historyKeys, hostKeys } from "@core/query-keys";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import {
  createSettingsStore,
  useSettings,
  useSettingsStatus,
} from "./settings-store";

describe("createSettingsStore", () => {
  it("drops a snapshot whose revision is not newer (D-050)", () => {
    const store = createSettingsStore();
    const base = { revision: 5 } as never;
    expect(store.getState().replace(base)).toBe(true);
    expect(store.getState().replace({ revision: 4 } as never)).toBe(false);
    expect(store.getState().replace({ revision: 5 } as never)).toBe(false);
    expect(store.getState().snapshot).toBe(base);
    expect(store.getState().replace({ revision: 6 } as never)).toBe(true);
  });

  it("is failed only while no snapshot has arrived", () => {
    const store = createSettingsStore();
    expect(store.getState().status).toBe("loading");
    store.getState().fail();
    expect(store.getState().status).toBe("failed");
    store.getState().replace({ revision: 1 } as never);
    expect(store.getState().status).toBe("ready");
    store.getState().fail();
    expect(store.getState().status).toBe("ready");
  });
});

function Interval() {
  const ms = useSettings((s) => s.sampling.interval_ms);
  const appearance = useSettings((s) => s.general.appearance);
  return (
    <p>
      interval {ms ?? "none"} {appearance}
    </p>
  );
}

describe("SettingsProvider", () => {
  it("mirrors settings-changed and invalidates by changed section", async () => {
    const { transport, queryClient } = renderWithProviders(<Interval />);
    await screen.findByText("interval 1000 system");

    const history = [...historyKeys.host(MOCK_HOST_ID), "probe"];
    const size = hostKeys.historySize(MOCK_HOST_ID);
    queryClient.setQueryData(history, 1);
    queryClient.setQueryData(size, 1);
    const invalidated = (key: readonly unknown[]) =>
      queryClient.getQueryState(key)?.isInvalidated;

    // Appearance changes no fetched data.
    await act(() =>
      transport.updateSettings({ general: { appearance: "dark" } })
    );
    await screen.findByText("interval 1000 dark");
    expect(invalidated(history)).toBe(false);
    expect(invalidated(size)).toBe(false);

    // The interval changes the size projection, not recorded history.
    await act(() =>
      transport.updateSettings({ sampling: { interval_ms: 2000 } })
    );
    await screen.findByText("interval 2000 dark");
    await waitFor(() => expect(invalidated(size)).toBe(true));
    expect(invalidated(history)).toBe(false);

    // Retention changes what history exists.
    await act(() =>
      transport.updateSettings({ history: { retention_days: 7 } })
    );
    await waitFor(() => expect(invalidated(history)).toBe(true));
  });

  it("recovers from a failed get_settings when settings-changed arrives", async () => {
    const transport = createMockTransport({ autoTick: false });
    transport.getSettings = () => Promise.reject(new Error("bridge down"));
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    onTestFinished(() => error.mockRestore());
    function Status() {
      const status = useSettingsStatus();
      const window = useSettings((s) => s.general.chart_window);
      return (
        <p>
          {status} {window ?? "none"}
        </p>
      );
    }
    renderWithProviders(<Status />, { transport });
    await screen.findByText("failed none");

    await act(() =>
      transport.updateSettings({ general: { chart_window: "1h" } })
    );
    await screen.findByText("ready 1h");
  });
});
