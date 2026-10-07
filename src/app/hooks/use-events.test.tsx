import type { Event } from "@core/generated/bindings";
import { createMockTransport, type MockTransport } from "@core/mock-transport";
import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { useEvents } from "./use-events";

const DAY = 86_400_000;

function Probe({
  spanMs = DAY,
  endMs = null,
}: {
  spanMs?: number;
  endMs?: number | null;
}) {
  const events = useEvents(spanMs, endMs);
  return <p>events {events.map((e) => e.detail.kind).join(",")}</p>;
}

/** A second list, mounted on demand, as the Power chart joins the Timeline. */
function LaterProbe() {
  const [shown, setShown] = useState(false);
  return shown ? (
    <Probe spanMs={DAY - 1} />
  ) : (
    <button type="button" onClick={() => setShown(true)}>
      mount
    </button>
  );
}

const FIXTURE = "events sustained_process,fans_ramped,alert,power_spike";
const WITH_PUSH = `${FIXTURE},power_spike`;

/**
 * Reads answer as if the event at `ts` had not committed yet, and wait for
 * `gate` (resolved at once by default) before answering.
 */
function staleReads(
  transport: MockTransport,
  ts: number,
  gate: () => Promise<void> = async () => {}
) {
  const read = transport.queryEvents.bind(transport);
  transport.queryEvents = async (...args) => {
    const answer = await read(...args);
    await gate();
    return answer.status === "ok"
      ? { ...answer, data: answer.data.filter((e) => e.ts_ms !== ts) }
      : answer;
  };
}

/** Let the query cache's batched notify (a 0 ms timer) render. */
const settle = () => act(() => new Promise((r) => setTimeout(r, 10)));

const eventCalls = (calls: { command: string }[]) =>
  calls.filter((c) => c.command === "query_events").length;

const spike = (ts: number): Event => ({
  ts_ms: ts,
  start_ms: ts - 10_000,
  processes: ["x264"],
  detail: {
    kind: "power_spike",
    component: "package",
    watts: 30,
    baseline_watts: 8,
  },
});

describe("useEvents", () => {
  it("reads the day's events once", async () => {
    const { transport } = renderWithProviders(<Probe />);
    await screen.findByText(
      "events sustained_process,fans_ramped,alert,power_spike"
    );
    expect(eventCalls(transport.calls)).toBe(1);
  });

  it("adds a pushed event without reading again", async () => {
    // Two listeners, as two components on a page hold: each merges the
    // push into the one cached list, which keeps it once.
    const { transport } = renderWithProviders(
      <>
        <Probe />
        <Probe />
      </>
    );
    await screen.findAllByText(/events sustained_process/);
    act(() => transport.recordEvent(spike(Date.now())));
    const after = await screen.findAllByText(
      "events sustained_process,fans_ramped,alert,power_spike,power_spike"
    );
    expect(after).toHaveLength(2);
    expect(eventCalls(transport.calls)).toBe(1);
  });

  it("keeps an event pushed while the first read is in flight", async () => {
    // The engine publishes once the commit is queued, so the first read can
    // answer from before the commit and resolve after the push.
    const transport = createMockTransport({ autoTick: false });
    const read = transport.queryEvents.bind(transport);
    let release = () => {};
    const held = new Promise<void>((resolve) => {
      release = resolve;
    });
    transport.queryEvents = async (...args) => {
      const answer = await read(...args);
      await held;
      return answer;
    };
    renderWithProviders(<Probe />, { transport });
    await vi.waitFor(() => expect(eventCalls(transport.calls)).toBe(1));
    act(() => transport.recordEvent(spike(Date.now())));
    release();
    await screen.findByText(
      "events sustained_process,fans_ramped,alert,power_spike,power_spike"
    );
    expect(eventCalls(transport.calls)).toBe(1);
  });

  it("keeps a pushed event when a read right after misses its commit", async () => {
    const { transport, queryClient } = renderWithProviders(<Probe />);
    await screen.findByText(FIXTURE);
    const ts = Date.now();
    staleReads(transport, ts);
    act(() => transport.recordEvent(spike(ts)));
    await act(() => queryClient.refetchQueries());
    await settle();
    expect(eventCalls(transport.calls)).toBe(2);
    expect(screen.getByText(WITH_PUSH)).toBeInTheDocument();
  });

  it("gives a list mounted just after a push the event its read missed", async () => {
    const { transport, user } = renderWithProviders(
      <>
        <Probe />
        <LaterProbe />
      </>
    );
    await screen.findByText(FIXTURE);
    const ts = Date.now();
    staleReads(transport, ts);
    act(() => transport.recordEvent(spike(ts)));
    await user.click(screen.getByRole("button", { name: "mount" }));
    await vi.waitFor(() => expect(eventCalls(transport.calls)).toBe(2));
    expect(await screen.findAllByText(WITH_PUSH)).toHaveLength(2);
  });

  it("keeps a pushed event when the read in flight is cancelled and rerun", async () => {
    const { transport, queryClient } = renderWithProviders(<Probe />);
    await screen.findByText(FIXTURE);
    const ts = Date.now();
    const gates: (() => void)[] = [];
    staleReads(
      transport,
      ts,
      () => new Promise<void>((resolve) => gates.push(resolve))
    );
    void queryClient.refetchQueries();
    await vi.waitFor(() => expect(gates).toHaveLength(1));
    act(() => transport.recordEvent(spike(ts)));
    // A second refetch cancels the first (cancelRefetch) and reads again.
    const rerun = queryClient.refetchQueries();
    await vi.waitFor(() => expect(gates).toHaveLength(2));
    for (const open of gates) open();
    await act(() => rerun);
    await settle();
    expect(screen.getByText(WITH_PUSH)).toBeInTheDocument();
  });

  it("says nothing when history is unavailable", async () => {
    const { transport } = renderWithProviders(<Probe />, {
      transportOptions: { scenarios: ["history-unavailable"] },
    });
    await vi.waitFor(() => expect(eventCalls(transport.calls)).toBe(1));
    expect(screen.getByText("events")).toBeInTheDocument();
  });
});
