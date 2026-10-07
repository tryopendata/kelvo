import type {
  HistoryPage,
  HistoryPoint,
  ProcessesAt,
} from "@core/generated/bindings";
import { createMockTransport, type MockTransport } from "@core/mock-transport";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { type HostStore, useHostStore } from "~/stores/host-store";
import { LANES } from "../_lib/lanes";
import {
  type TimelineData,
  type TimelineView,
  useProcessesAt,
  useTimelineData,
} from "./use-timeline-data";

const MIN = 60_000;
/** Mid-bucket, so the newest row is not on a 10 s boundary. */
const NOW = 1_760_000_005_000;
const CPU = LANES.filter((l) => l.id === "cpu");
const LIVE_1H: TimelineView = { span: "1h", endMs: null };

const bucket = (t: number) => Math.floor(t / 10_000) * 10_000;

let latest: TimelineData | null = null;
let storeRef: HostStore | null = null;

function TimelineProbe() {
  storeRef = useHostStore();
  latest = useTimelineData(LIVE_1H, CPU);
  return <p>{latest.loading ? "loading" : "loaded"}</p>;
}

const cpuBuckets = () => latest?.lanes[0]?.series["cpu.total"] ?? [];
const hasBucket = (t: number) => cpuBuckets().some((b) => b.t === bucket(t));

function state() {
  if (!storeRef) throw new Error("no store");
  return storeRef.getState();
}

beforeEach(() => {
  // Date only: the hook's mount time and the history request read the wall
  // clock, and must agree with the mock's frame times. Timers stay real.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  latest = null;
  storeRef = null;
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useTimelineData at the live edge (D-092)", () => {
  const historyCalls = (t: MockTransport) =>
    t.calls.filter((c) => c.command === "query_history");

  it("draws history through the newest closed bucket and reads it again when a bucket closes", async () => {
    let clock = NOW;
    const transport = createMockTransport({
      now: () => clock,
      historyRows: 600,
      autoTick: false,
    });
    renderWithProviders(<TimelineProbe />, { transport });
    await screen.findByText("loaded");
    await waitFor(() => expect(latest?.toMs).toBe(bucket(NOW)));
    // Rust answers through now: the last closed bucket is there, and the
    // open one is past the window's end.
    expect(hasBucket(NOW - 10_000)).toBe(true);
    expect(hasBucket(NOW)).toBe(false);
    const calls = historyCalls(transport).length;

    // Ticks inside the open bucket read nothing.
    await act(async () => {
      clock += 1000;
      vi.setSystemTime(clock);
      transport.tick();
    });
    expect(historyCalls(transport).length).toBe(calls);

    // The bucket closes: the newest slots are read again and the bucket is
    // drawn. Only from the slot open at the first read, not the whole hour.
    await act(async () => {
      for (let i = 0; i < 10; i++) {
        clock += 1000;
        vi.setSystemTime(clock);
        transport.tick();
      }
    });
    await waitFor(() => expect(hasBucket(NOW)).toBe(true));
    expect(latest?.toMs).toBe(bucket(NOW) + 10_000);
    const reads = historyCalls(transport);
    expect(reads.length).toBe(calls + CPU.length);
    const tail = reads.at(-1)?.args[0] as {
      from_ms: number;
      to_ms: number;
      tier: string;
      max_points: number;
    };
    expect(tail).toMatchObject({
      from_ms: bucket(NOW),
      to_ms: bucket(NOW) + 20_000,
      tier: "s10",
      max_points: 2,
    });
    // The hour before it is still drawn.
    expect(hasBucket(NOW - 50 * MIN)).toBe(true);
  });

  it("does not draw the bucket open at the read as closed while the next read is pending", async () => {
    let clock = NOW;
    const transport = createMockTransport({
      now: () => clock,
      historyRows: 600,
      autoTick: false,
    });
    renderWithProviders(<TimelineProbe />, { transport });
    await screen.findByText("loaded");
    await waitFor(() => expect(latest?.toMs).toBe(bucket(NOW)));

    const held: (() => void)[] = [];
    const read = transport.queryHistory.bind(transport);
    transport.queryHistory = (req) =>
      new Promise((resolve) => held.push(() => resolve(read(req))));
    await act(async () => {
      for (let i = 0; i < 10; i++) {
        clock += 1000;
        vi.setSystemTime(clock);
        transport.tick();
      }
    });
    // The window's end moved past the bucket; the page has it partial.
    expect(latest?.toMs).toBe(bucket(NOW) + 10_000);
    expect(held.length).toBe(CPU.length);
    expect(hasBucket(NOW)).toBe(false);
    expect(hasBucket(NOW - 10_000)).toBe(true);

    await act(async () => {
      for (const release of held) release();
    });
    await waitFor(() => expect(hasBucket(NOW)).toBe(true));
  });

  it("does not read again when the first page comes back at another width than asked", async () => {
    let clock = NOW;
    const transport = createMockTransport({
      now: () => clock,
      historyRows: 600,
      autoTick: false,
    });
    // A store without 10 s buckets for the hour: Auto answers in minutes.
    // The first read lands after the backfill, so the edge is first seen at
    // the 10 s width asked for.
    const read = transport.queryHistory.bind(transport);
    const held: (() => void)[] = [];
    transport.queryHistory = (req) =>
      new Promise((resolve) =>
        held.push(() => resolve(read({ ...req, tier: "m1" })))
      );
    renderWithProviders(<TimelineProbe />, { transport });
    await waitFor(() => expect(latest?.toMs).toBe(bucket(NOW)));
    await waitFor(() => expect(held.length).toBe(CPU.length));
    await act(async () => {
      for (const release of held) release();
    });
    await screen.findByText("loaded");
    await waitFor(() => expect(latest?.bucketMs).toBe(MIN));
    await waitFor(() => expect(latest?.toMs).toBe(Math.floor(NOW / MIN) * MIN));
    // Inside the open minute (NOW is 25 s into it).
    await act(async () => {
      for (let i = 0; i < 5; i++) {
        clock += 1000;
        vi.setSystemTime(clock);
        transport.tick();
      }
    });
    expect(held.length).toBe(CPU.length);
  });

  it("takes each metric's hold from Rust, the most of its label sets", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    transport.queryHistory = async (req) => {
      const page: HistoryPage = {
        tier: "s10",
        bucket_ms: 10_000,
        series: req.selectors.flatMap((s, i) =>
          [0, 1].map((n) => ({
            key: { metric: s.metric, labels: [["n", String(n)]] },
            points: [] as HistoryPoint[],
            hold_ms: 10_000 + i * 1000 + n * 50_000,
          }))
        ),
        gaps: [],
      };
      return { status: "ok", data: page };
    };
    renderWithProviders(<TimelineProbe />, { transport });
    await screen.findByText("loaded");
    const metric = CPU[0]?.metrics[0]?.metric ?? "";
    expect(latest?.lanes[0]?.holds[metric]).toBe(60_000);
  });
});

describe("useTimelineData beyond the 7-day minute window (D-076)", () => {
  const DAY = 24 * 60 * MIN;
  const QUARTER = 15 * MIN;

  function OldDayProbe({ endMs }: { endMs: number }) {
    latest = useTimelineData({ span: "24h", endMs }, CPU);
    return <p>{latest.loading ? "loading" : "loaded"}</p>;
  }

  it("draws a 24h range older than 7 days from 15-minute history", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    const endMs = NOW - 10 * DAY;
    renderWithProviders(<OldDayProbe endMs={endMs} />, { transport });
    await screen.findByText("loaded");

    expect(latest?.tier).toBe("m15");
    expect(latest?.bucketMs).toBe(QUARTER);
    expect(latest?.resolution).toBe("15 min avg");
    const ts = cpuBuckets().map((b) => b.t);
    // A day of quarters, less the ones the mock's overnight gap cuts out.
    expect(ts.length).toBeGreaterThan(48);
    for (const t of ts) {
      expect(t % QUARTER).toBe(0);
      expect(t).toBeGreaterThanOrEqual(endMs - DAY - QUARTER);
      expect(t).toBeLessThan(endMs);
    }
  });

  it("keeps minutes for a 24h range inside the last 7 days", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    renderWithProviders(<OldDayProbe endMs={NOW - 3 * DAY} />, { transport });
    await screen.findByText("loaded");
    expect(latest?.tier).toBe("m1");
    expect(latest?.resolution).toBe("1 min avg");
  });
});

describe("useTimelineData at 7d and 30d (v1.1, D-076)", () => {
  const DAY = 24 * 60 * MIN;
  /** An 820 px plot: what a 1280 px dashboard window leaves the lanes. */
  const PLOT_PX = 820;
  let renders = 0;

  function LongProbe({ view }: { view: TimelineView }) {
    renders += 1;
    storeRef = useHostStore();
    latest = useTimelineData(view, CPU, PLOT_PX);
    return <p>{latest.loading ? "loading" : "loaded"}</p>;
  }

  const historyCalls = (t: MockTransport) =>
    t.calls.filter((c) => c.command === "query_history");

  it("reads 7d ending now from minutes merged to 10-minute buckets", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    renderWithProviders(<LongProbe view={{ span: "7d", endMs: null }} />, {
      transport,
    });
    await screen.findByText("loaded");

    expect(latest?.tier).toBe("m1");
    expect(latest?.bucketMs).toBe(10 * MIN);
    expect(latest?.resolution).toBe("10 min avg");
    const req = historyCalls(transport)[0]?.args[0] as {
      max_points: number;
      tier: string;
    };
    expect(req.tier).toBe("auto");
    expect(req.max_points).toBeLessThanOrEqual(2 * PLOT_PX);
    const ts = cpuBuckets().map((b) => b.t);
    expect(ts.length).toBeGreaterThan(500);
    for (const t of ts) expect(t % (10 * MIN)).toBe(0);
  });

  it("reads 30d from quarters merged to 30-minute buckets, with every night a gap", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    renderWithProviders(<LongProbe view={{ span: "30d", endMs: null }} />, {
      transport,
    });
    await screen.findByText("loaded");

    expect(latest?.tier).toBe("m15");
    expect(latest?.bucketMs).toBe(30 * MIN);
    expect(latest?.resolution).toBe("30 min avg");
    const gaps = latest?.gaps ?? [];
    expect(gaps.filter((g) => g.reason === "sleep").length).toBeGreaterThan(25);
    // Nothing drawn inside a gap: the slots there are absent, not zero.
    for (const b of cpuBuckets()) {
      const inside = gaps.some(
        (g) => b.t >= g.start_ms && b.t < (g.end_ms ?? Number.POSITIVE_INFINITY)
      );
      expect(inside).toBe(false);
    }
  });

  it("does not re-read or re-render 30d on live ticks inside one bucket", async () => {
    let clock = NOW;
    const transport = createMockTransport({
      now: () => clock,
      autoTick: false,
      historyRows: 600,
    });
    renderWithProviders(<LongProbe view={{ span: "30d", endMs: null }} />, {
      transport,
    });
    await screen.findByText("loaded");
    // The live edge lands once the backfill does.
    await waitFor(() =>
      expect(latest?.toMs).toBe(Math.floor(NOW / (30 * MIN)) * 30 * MIN)
    );
    const calls = historyCalls(transport).length;
    const lanes = latest?.lanes;
    const before = renders;
    const lastBefore = state().lastTsMs ?? 0;

    // Five minutes of 1 s ticks, all inside the open 30-minute bucket
    // (NOW is 23 minutes into it).
    await act(async () => {
      for (let i = 0; i < 300; i++) {
        clock += 1000;
        transport.tick();
      }
    });
    // The ticks did land in the ring.
    expect((state().lastTsMs ?? 0) - lastBefore).toBeGreaterThanOrEqual(
      5 * MIN
    );

    expect(historyCalls(transport).length).toBe(calls);
    expect(latest?.lanes).toBe(lanes);
    expect(renders).toBe(before);
  });

  it("draws 7d stepped back past the minute window at the quarters it gets", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    const endMs = NOW - 10 * DAY;
    renderWithProviders(<LongProbe view={{ span: "7d", endMs }} />, {
      transport,
    });
    await screen.findByText("loaded");
    // Asked for 10-minute points; only quarters exist that far back.
    expect(latest?.tier).toBe("m15");
    expect(latest?.bucketMs).toBe(15 * MIN);
    expect(latest?.resolution).toBe("15 min avg");
  });

  it("steps back a whole range and reads that window", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    const endMs = Math.floor(NOW / (30 * MIN)) * 30 * MIN - 30 * DAY;
    renderWithProviders(<LongProbe view={{ span: "30d", endMs }} />, {
      transport,
    });
    await screen.findByText("loaded");
    expect(latest?.toMs).toBe(endMs);
    expect(latest?.fromMs).toBe(endMs - 30 * DAY);
    const req = historyCalls(transport)[0]?.args[0] as {
      from_ms: number;
      to_ms: number;
    };
    expect(req.to_ms).toBe(endMs);
    expect(req.from_ms).toBe(endMs - 30 * DAY);
  });
});

describe("useProcessesAt", () => {
  let setBucket: (t: number) => void = () => {};
  let setWidth: (ms: number) => void = () => {};

  function ProcessesProbe({ initial }: { initial: number }) {
    const [t, setT] = useState(initial);
    const [width, setW] = useState(10_000);
    setBucket = setT;
    setWidth = setW;
    const at = useProcessesAt(t, width);
    const label =
      at === undefined ? "pending" : at === null ? "none" : `rows@${at.ts_ms}`;
    return (
      <p>
        {t} {label}
      </p>
    );
  }

  const SNAPSHOT: ProcessesAt = {
    ts_ms: 0,
    resolution: "snapshot",
    rows: [],
  };

  /**
   * `null` for every bucket until `committed` is set, as the store answers;
   * a snapshot stamped with the asked time after.
   */
  function setup(initial: number, committed = false) {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    const state = { committed };
    transport.queryProcessesAt = async (_host, tMs) => {
      transport.calls.push({ command: "query_processes_at", args: [tMs] });
      return {
        status: "ok",
        data: state.committed ? { ...SNAPSHOT, ts_ms: tMs } : null,
      };
    };
    renderWithProviders(<ProcessesProbe initial={initial} />, { transport });
    const askedAt = () =>
      transport.calls
        .filter((c) => c.command === "query_processes_at")
        .map((c) => c.args[0]);
    const asked = () => askedAt().length;
    return { state, asked, askedAt };
  }

  it("asks again for a bucket newer than the last commit that had nothing yet", async () => {
    const recent = bucket(NOW - 1 * MIN);
    const old = bucket(NOW - 2 * 60 * MIN);
    const { state, asked } = setup(recent);
    await screen.findByText(`${recent} none`);
    expect(asked()).toBe(1);

    // The writer commits; the cursor leaves and comes back.
    state.committed = true;
    act(() => setBucket(old));
    await screen.findByText(`${old} rows@${old + 5_000}`);
    act(() => setBucket(recent));
    await screen.findByText(`${recent} rows@${recent + 5_000}`);
    expect(asked()).toBe(3);
  });

  it("keeps an answer for a bucket the store has committed", async () => {
    const old = bucket(NOW - 2 * 60 * MIN);
    const other = bucket(NOW - 3 * 60 * MIN);
    const { asked } = setup(old);
    await screen.findByText(`${old} none`);
    act(() => setBucket(other));
    await screen.findByText(`${other} none`);
    act(() => setBucket(old));
    await screen.findByText(`${old} none`);
    expect(asked()).toBe(2);
  });

  it("asks again when a range switch changes the bucket width at the same start", async () => {
    // A 15-minute boundary is also a 1-minute and a 10 s boundary.
    const start = Math.floor((NOW - 2 * 60 * MIN) / (15 * MIN)) * 15 * MIN;
    const { askedAt } = setup(start, true);
    await screen.findByText(`${start} rows@${start + 5_000}`);

    act(() => setWidth(15 * MIN));
    await screen.findByText(`${start} rows@${start + 7.5 * MIN}`);
    expect(askedAt()).toEqual([start + 5_000, start + 7.5 * MIN]);
  });

  it("never asks for the old start at the new width while a range switch settles", async () => {
    const start = Math.floor((NOW - 2 * 60 * MIN) / (15 * MIN)) * 15 * MIN;
    const next = start - 15 * MIN;
    const { askedAt } = setup(start, true);
    await screen.findByText(`${start} rows@${start + 5_000}`);

    // A range switch moves the cursor's bucket and its width in one render.
    act(() => {
      setBucket(next);
      setWidth(15 * MIN);
    });
    await screen.findByText(`${next} rows@${next + 7.5 * MIN}`);
    expect(askedAt()).toEqual([start + 5_000, next + 7.5 * MIN]);
  });
});
