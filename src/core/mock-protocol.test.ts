import type { LiveMsg } from "@core/generated/bindings";
import fixture from "@tests/fixtures/live-protocol.json";
import { MOCK_HOST_ID } from "./mock/fixtures";
import { createMockTransport } from "./mock-transport";

function trace(messages: LiveMsg[]) {
  return messages
    .filter((m) => m.kind !== "holds")
    .map((message) => {
      switch (message.kind) {
        case "backfill":
        case "backfill_earlier":
          return {
            kind: message.kind,
            start_ms: message.start_ms,
            rows: message.rows.length,
          };
        case "frame":
          return { kind: message.kind, ts_ms: message.ts_ms };
        default:
          return { kind: message.kind };
      }
    });
}

// Segment boundaries can differ with catalog holds. Compare every older sample
// in delivery order; this still catches omissions, overlaps and reversed chunks.
interface TraceEntry {
  kind: string;
  rows?: number;
  start_ms?: number;
  ts_ms?: number;
}
function delivery(samples: TraceEntry[]): TraceEntry[] {
  return samples.flatMap<TraceEntry>((message) => {
    if (
      message.kind !== "backfill_earlier" ||
      message.rows === undefined ||
      message.start_ms === undefined
    )
      return [message];
    const { rows, start_ms } = message;
    expect(rows).toBeLessThanOrEqual(600);
    return Array.from({ length: rows }, (_, i) => ({
      kind: message.kind,
      ts_ms: start_ms + (rows - 1 - i) * 1000,
    }));
  });
}

it("matches the Rust registry's ordering, backfill boundaries, disconnect and typed failure", async () => {
  vi.useFakeTimers();
  let now = 1_500_000;
  const transport = createMockTransport({ now: () => now, historyRows: 1500 });
  try {
    const messages: LiveMsg[] = [];
    const sub = await transport.subscribeLive(
      MOCK_HOST_ID,
      (m) => messages.push(m),
      {
        backfillMs: 3_600_000,
        series: [{ metric: "cpu.total", labels: [] }],
      }
    );
    expect(trace(messages.splice(0))).toEqual(fixture.initial);
    now += 1000;
    transport.tick();
    await vi.runAllTimersAsync();
    expect(delivery(trace(messages.splice(0)))).toEqual(
      delivery(fixture.after_frame)
    );
    sub.unsubscribe();
    now += 1000;
    transport.tick();
    await vi.runAllTimersAsync();
    expect(trace(messages)).toEqual(fixture.disconnected);
    expect(
      await transport.getHost("00000000-0000-0000-0000-000000000000")
    ).toEqual(fixture.typed_failure);
  } finally {
    transport.dispose();
    vi.useRealTimers();
  }
});

it("matches Rust cancellation while older backfill is still pending", async () => {
  vi.useFakeTimers();
  const transport = createMockTransport({
    now: () => 1_500_000,
    historyRows: 1500,
  });
  try {
    const messages: LiveMsg[] = [];
    const sub = await transport.subscribeLive(
      MOCK_HOST_ID,
      (m) => messages.push(m),
      { backfillMs: 3_600_000 }
    );
    messages.length = 0;
    sub.unsubscribe();
    await vi.advanceTimersByTimeAsync(2000);
    transport.tick();
    await vi.runAllTimersAsync();
    expect(trace(messages)).toEqual(fixture.cancelled);
  } finally {
    transport.dispose();
    vi.useRealTimers();
  }
});
