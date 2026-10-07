import type { TimeRange } from "@core/brush";
import type { NetworkByApp } from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import {
  PROBE_FAST_READS,
  PROBE_POLL_MS,
  useCompleteEdge,
  useNetworkByApp,
} from "./use-network-by-app";

const T = 1_700_000_000_000; // a 10 s edge
const S = 1000;
const RANGE: TimeRange = { fromMs: T - 20 * S, toMs: T };

/** A transport whose answers are complete up to `complete.ms`. */
function setup() {
  const transport = createMockTransport({ now: () => T, autoTick: false });
  const complete = { ms: T };
  let calls = 0;
  transport.queryNetworkByApp = async (_host, fromMs, toMs) => {
    calls++;
    const data: NetworkByApp = {
      from_ms: fromMs,
      to_ms: toMs,
      complete_to_ms: Math.min(toMs, complete.ms),
      resolution_ms: 10_000,
      measured_ms: toMs - fromMs,
      coverage: [{ from_ms: fromMs, to_ms: toMs, tier: "s10" }],
      apps: [],
      other_apps_rx_bytes: 0,
      other_apps_tx_bytes: 0,
      iface_rx_bytes: 0,
      iface_tx_bytes: 0,
      overhead_rx_bytes: 0,
      overhead_tx_bytes: 0,
      system_rx_bytes: 0,
      system_tx_bytes: 0,
      clamped: false,
    };
    return { status: "ok", data };
  };
  return { transport, complete, calls: () => calls };
}

function Probe({ range }: { range: TimeRange }) {
  const { data } = useNetworkByApp(range);
  return <output>{data ? `complete ${data.complete_to_ms - T}` : "…"}</output>;
}

/** Moves the open edge `Edge` probes from (the newest sample's bucket). */
const openEdge = { set: (_ms: number) => {} };

function Edge() {
  const [at, setAt] = useState(T);
  openEdge.set = setAt;
  const edge = useCompleteEdge(at);
  return <output>{edge === null ? "…" : `edge ${edge - T}`}</output>;
}

const tick = (ms: number) => act(() => vi.advanceTimersByTimeAsync(ms));

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval"],
    shouldAdvanceTime: true,
  });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useNetworkByApp liveness (D-089)", () => {
  it("reads a range with an open bucket again every 10 s until it is complete", async () => {
    const { transport, complete, calls } = setup();
    complete.ms = T - 10 * S;
    renderWithProviders(<Probe range={RANGE} />, { transport });
    expect(await screen.findByText(`complete ${-10 * S}`)).toBeVisible();
    expect(calls()).toBe(1);
    await tick(10 * S);
    expect(calls()).toBe(2);
    // The engine closes the bucket: the next read is final.
    complete.ms = T;
    await tick(10 * S);
    expect(await screen.findByText("complete 0")).toBeVisible();
    const final = calls();
    await tick(60 * S);
    expect(calls()).toBe(final);
  });

  it("reads a complete range once, and not again on remount", async () => {
    const { transport, calls } = setup();
    const { unmount, queryClient } = renderWithProviders(
      <Probe range={RANGE} />,
      { transport }
    );
    expect(await screen.findByText("complete 0")).toBeVisible();
    await tick(60 * S);
    expect(calls()).toBe(1);
    unmount();
    renderWithProviders(<Probe range={RANGE} />, { transport, queryClient });
    expect(await screen.findByText("complete 0")).toBeVisible();
    expect(calls()).toBe(1);
  });

  it("probes the bucket before the open edge again soon, not at the next edge", async () => {
    const { transport, complete, calls } = setup();
    // Read just past the edge: the per-app stream has not reported past it.
    complete.ms = T - 10 * S;
    renderWithProviders(<Edge />, { transport });
    expect(await screen.findByText(`edge ${-10 * S}`)).toBeVisible();
    complete.ms = T;
    await tick(PROBE_POLL_MS);
    expect(calls()).toBe(2);
    expect(await screen.findByText("edge 0")).toBeVisible();
    const final = calls();
    await tick(60 * S);
    expect(calls()).toBe(final);
  });

  it("a stalled stream is probed about once per 10 s, not every 2 s", async () => {
    const { transport, complete, calls } = setup();
    // The per-app stream has stalled: buckets close only by grace, 30 s behind.
    complete.ms = T - 30 * S;
    renderWithProviders(<Edge />, { transport });
    expect(await screen.findByText(`edge ${-30 * S}`)).toBeVisible();
    await tick(10 * S);
    act(() => openEdge.set(T + 10 * S));
    await tick(10 * S);
    act(() => openEdge.set(T + 20 * S));
    await tick(10 * S);
    // A read per edge and one per 10 s: about 6 over 30 s, not 15.
    expect(calls()).toBeLessThanOrEqual(6);
  });

  it("a bucket that stays open while the edge holds (paused) slows to every 10 s", async () => {
    const { transport, complete, calls } = setup();
    // One bucket behind and the open edge never moves: sampling is paused.
    complete.ms = T - 10 * S;
    renderWithProviders(<Edge />, { transport });
    expect(await screen.findByText(`edge ${-10 * S}`)).toBeVisible();
    await tick(60 * S);
    // The fast phase ends after PROBE_FAST_READS answers, then one per 10 s.
    expect(calls()).toBeLessThanOrEqual(PROBE_FAST_READS + 6);
  });
});
