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
