/**
 * The live subscription against the D-066 channel: a channel that ends
 * silently is resubscribed (review #14), process interest follows the
 * stream, and the dashboard's hour arrives through earlier chunks.
 */
import { MOCK_HOST_ID } from "@core/mock/fixtures";
import { createMockTransport, type MockTransport } from "@core/mock-transport";
import { act, render } from "@testing-library/react";
import { useProcessInterest } from "~/hooks/use-process-interest";
import { useWindowSeries } from "~/hooks/use-window-series";
import { TransportProvider } from "~/lib/transport-context";
import { HostStoreProvider, useHost } from "./host-store";

const HOUR = 3_600_000;

function Probe() {
  const connection = useHost((s) => s.connection);
  const stale = useHost((s) => s.stale);
  return (
    <span data-testid="state">
      {connection} {String(stale)}
    </span>
  );
}

function Interested() {
  useProcessInterest({ limit: 5, sort: ["cpu"], period_ms: 5000 });
  return null;
}

function mount(transport: MockTransport, backfillMs?: number, extra = <></>) {
  return render(
    <TransportProvider transport={transport}>
      <HostStoreProvider hostId={MOCK_HOST_ID} backfillMs={backfillMs}>
        <Probe />
        {extra}
      </HostStoreProvider>
    </TransportProvider>
  );
}

const subscribes = (t: MockTransport) =>
  t.calls.filter((c) => c.command === "subscribe_live");

/** Advance fake time one interval and produce one frame. */
async function tick(t: MockTransport) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(1000);
    t.tick();
  });
}

async function wait(ms: number) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

function setVisibility(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => state,
  });
  document.dispatchEvent(new Event("visibilitychange"));
}

describe("a live channel that ends silently (#14)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });
  afterEach(() => {
    setVisibility("visible");
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("resubscribes after three more stale intervals, asking for the span missed", async () => {
    const t = createMockTransport({ autoTick: false });
    const { getByTestId } = mount(t);
    await wait(0);
    await tick(t);
    await tick(t);
    expect(getByTestId("state").textContent).toBe("live false");

    // Frames stop: stale after three intervals, no resubscribe yet.
    await wait(3000);
    expect(getByTestId("state").textContent).toBe("live true");
    await wait(2900);
    expect(subscribes(t)).toHaveLength(1);

    await wait(100);
    expect(subscribes(t)).toHaveLength(2);
    // About six seconds missed plus two intervals of slack, not the minute.
    const backfill = subscribes(t)[1]?.args[1] as number;
    expect(backfill).toBeGreaterThanOrEqual(6000);
    expect(backfill).toBeLessThan(10_000);
    expect(console.warn).toHaveBeenCalledWith(
      "[live] no frames while visible; resubscribing",
      expect.objectContaining({ hostId: MOCK_HOST_ID, window: "dashboard" })
    );

    // Frames resume on the new channel.
    await tick(t);
    expect(getByTestId("state").textContent).toBe("live false");
    t.dispose();
  });

  it("shows reconnecting until the new subscription answers", async () => {
    const t = createMockTransport({ autoTick: false });
    const { getByTestId } = mount(t);
    await wait(0);
    await tick(t);
    // The resubscribe hangs: the pill reads Reconnecting meanwhile.
    t.subscribeLive = () => new Promise(() => {});
    await wait(6000);
    expect(getByTestId("state").textContent).toBe("reconnecting true");
    t.dispose();
  });

  it("does not resubscribe while hidden or paused", async () => {
    const hidden = createMockTransport({ autoTick: false });
    const a = mount(hidden);
    await wait(0);
    await tick(hidden);
    setVisibility("hidden");
    await wait(60_000);
    expect(subscribes(hidden)).toHaveLength(1);
    a.unmount();
    setVisibility("visible");

    const paused = createMockTransport({
      autoTick: false,
      scenarios: ["paused"],
    });
    mount(paused);
    await wait(60_000);
    expect(subscribes(paused)).toHaveLength(1);
  });

  it("re-sends process interest with the new stream", async () => {
    const t = createMockTransport({ autoTick: false });
    mount(t, undefined, <Interested />);
    await wait(0);
    await tick(t);
    await wait(6000);
    const interest = t.calls
      .filter((c) => c.command === "set_process_interest")
      .map((c) => c.args[3]);
    expect(interest).toEqual([1, 2]);
    expect(t.processInterest()?.stream).toBe(2);
  });
});

describe("no frames by design is not stale (#13)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(console, "warn").mockImplementation(() => {});
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("neither goes stale nor resubscribes while the display sleeps, and fills the gap on wake", async () => {
    const t = createMockTransport({ autoTick: false });
    const { getByTestId } = mount(t, HOUR, <LastTwoMinutes />);
    await wait(0);
    await tick(t);
    act(() => t.setDisplayIdle(true));
    for (let i = 0; i < 120; i++) await tick(t);
    expect(getByTestId("state").textContent).toBe("live false");
    expect(subscribes(t)).toHaveLength(1);

    act(() => t.setDisplayIdle(false));
    await wait(0);
    await tick(t);
    expect(getByTestId("state").textContent).toBe("live false");
    // The two asleep minutes arrived as backfill: no hole at the end.
    expect(last2m.every((v) => v !== null)).toBe(true);
    expect(subscribes(t)).toHaveLength(1);
    t.dispose();
  });

  it("measures stale in frame periods, not base ticks", async () => {
    const t = createMockTransport({ autoTick: false });
    const { getByTestId } = mount(t);
    await wait(0);
    await tick(t);
    // A channel thinned to one frame per 10 s (a v2 board's minimum period).
    act(() =>
      t.push({
        kind: "status",
        interval_ms: 1000,
        frame_period_ms: 10_000,
        paused: false,
        display_idle: false,
        on_battery: false,
        performance: "off",
        power_source: "adapter",
        primary_iface: null,
      })
    );
    await wait(29_000);
    expect(getByTestId("state").textContent).toBe("live false");
    expect(subscribes(t)).toHaveLength(1);
    await wait(1000);
    expect(getByTestId("state").textContent).toBe("live true");
    t.dispose();
  });
});

let last2m: (number | null)[] = [];
function LastTwoMinutes() {
  last2m = useWindowSeries(["cpu.total"], 120_000).values["cpu.total"] ?? [];
  return null;
}

describe("the dashboard's hour of ring", () => {
  afterEach(() => vi.useRealTimers());

  let first: number | null | undefined;
  function HourProbe() {
    first = useWindowSeries(["cpu.total"], HOUR).values["cpu.total"]?.[0];
    return null;
  }

  it("fills the hour once the earlier chunks arrive", async () => {
    vi.useFakeTimers();
    const t = createMockTransport({ autoTick: false, historyRows: 3600 });
    mount(t, HOUR, <HourProbe />);
    await wait(0);
    // Two minutes before the first frame: the hour's first bucket is empty.
    expect(first).toBeNull();
    // The chunks follow the first frame, one per task.
    await tick(t);
    await wait(100);
    expect(first).toEqual(expect.any(Number));
    t.dispose();
  });
});
