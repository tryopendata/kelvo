import { MOCK_HOST_ID } from "@core/mock/fixtures";
import { createMockTransport } from "@core/mock-transport";
import type { Transport } from "@core/transport";
import { act, render } from "@testing-library/react";
import { TransportProvider } from "~/lib/transport-context";
import { HostStoreProvider, useHost } from "./host-store";

function setVisibility(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => state,
  });
  document.dispatchEvent(new Event("visibilitychange"));
}

function Stale() {
  return <span data-testid="stale">{String(useHost((s) => s.stale))}</span>;
}

function mount(transport: Transport) {
  return render(
    <TransportProvider transport={transport}>
      <HostStoreProvider hostId={MOCK_HOST_ID}>
        <Stale />
      </HostStoreProvider>
    </TransportProvider>
  );
}

describe("live subscription while the window is hidden", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(console, "error").mockImplementation(() => {});
  });
  afterEach(() => {
    setVisibility("visible");
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("does not retry a failed subscribe until the window shows", async () => {
    const mock = createMockTransport({ autoTick: false });
    const subscribeLive = vi.fn(() => Promise.reject(new Error("closed")));
    mount({ ...mock, subscribeLive });
    await act(async () => {});
    expect(subscribeLive).toHaveBeenCalledTimes(1);

    setVisibility("hidden");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(60_000);
    });
    expect(subscribeLive).toHaveBeenCalledTimes(1);

    await act(async () => setVisibility("visible"));
    expect(subscribeLive).toHaveBeenCalledTimes(2);
  });

  it("does not mark a hidden window stale when frames stop", async () => {
    const mock = createMockTransport({ autoTick: false });
    const { getByTestId } = mount(mock);
    await act(async () => {});
    setVisibility("hidden");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(getByTestId("stale").textContent).toBe("false");

    // Shown: frames get three intervals from now before it reads stale.
    await act(async () => setVisibility("visible"));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_900);
    });
    expect(getByTestId("stale").textContent).toBe("false");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(200);
    });
    expect(getByTestId("stale").textContent).toBe("true");
  });
});

describe("subscription setup races", () => {
  it.each(["paused", "display_idle"] as const)(
    "reconnects a replaced remote even while %s disables the watchdog",
    async (state) => {
      const mock = createMockTransport({
        autoTick: false,
        scenarios: ["multi-host"],
      });
      const host = (await mock.listHosts()).find((h) => !h.is_local);
      if (!host) throw new Error("Missing remote fixture");
      let subscriptions = 0;
      const subscribeLive: Transport["subscribeLive"] = (
        id,
        callback,
        options
      ) => {
        subscriptions++;
        return mock.subscribeLive(
          id,
          (msg) =>
            callback(
              msg.kind === "status"
                ? { ...msg, [state]: subscriptions === 1 }
                : msg
            ),
          options
        );
      };
      const transport = { ...mock, subscribeLive };
      let paused = false;
      function Probe() {
        paused = useHost((s) => s.status?.[state] ?? false);
        return <Stale />;
      }
      const view = render(
        <TransportProvider transport={transport}>
          <HostStoreProvider hostId={host.id}>
            <Probe />
          </HostStoreProvider>
        </TransportProvider>
      );
      await act(async () => {});
      expect(paused).toBe(true);
      expect(subscriptions).toBe(1);
      await act(() => mock.emitHostsChanged());
      expect(subscriptions).toBe(2);
      expect(paused).toBe(false);
      view.unmount();
      mock.dispose();
    }
  );
  it("releases a subscription whose promise resolves after unmount", async () => {
    const mock = createMockTransport();
    let finish: (() => void) | undefined;
    const gate = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const subscribeLive: Transport["subscribeLive"] = async (...args) => {
      const subscription = await mock.subscribeLive(...args);
      await gate;
      return subscription;
    };
    const view = mount({ ...mock, subscribeLive });
    await act(async () => {});
    expect(mock.subscriberCount()).toBe(1);
    view.unmount();
    await act(async () => finish?.());
    expect(mock.subscriberCount()).toBe(0);
    mock.dispose();
  });

  it("does not retry when a pending subscription rejects after unmount", async () => {
    vi.useFakeTimers();
    const mock = createMockTransport();
    let fail: ((reason: Error) => void) | undefined;
    const pending = new Promise<never>((_resolve, reject) => {
      fail = reject;
    });
    const subscribeLive = vi.fn(() => pending);
    try {
      const view = mount({ ...mock, subscribeLive });
      view.unmount();
      await act(async () => fail?.(new Error("channel closed during setup")));
      await act(async () => vi.advanceTimersByTimeAsync(10_000));
      expect(subscribeLive).toHaveBeenCalledTimes(1);
      expect(mock.subscriberCount()).toBe(0);
    } finally {
      mock.dispose();
      vi.useRealTimers();
    }
  });
});
