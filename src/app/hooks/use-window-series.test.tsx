import { RING_MAX_ROWS } from "@core/generated/bindings";
import { seriesWindow } from "@core/live-state";
import { seriesKey } from "@core/series-key";
import { bucketAverages, bucketIndex } from "@core/series-stats";
import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { type HostStore, useHostStore } from "~/stores/host-store";
import { useRingBuckets } from "./use-ring";
import { useWindowSeries, type WindowSeries } from "./use-window-series";

const KEYS = ["cpu.total", "cpu.system"];
const HOUR = 3_600_000;
/** The per-core heatmap's buckets at the 15m default: 15 s columns. */
const HEAT_MS = 15_000;
const HEAT_COUNT = 60;

let latest: WindowSeries | null = null;
let heat: ReturnType<typeof useRingBuckets> | null = null;
let storeRef: HostStore | null = null;

function Probe({ windowMs }: { windowMs: number }) {
  storeRef = useHostStore();
  latest = useWindowSeries(KEYS, windowMs);
  heat = useRingBuckets(KEYS, HEAT_MS, HEAT_COUNT);
  return <p>points {latest.values["cpu.total"]?.length}</p>;
}

/** Frame timestamps on a fake clock, one interval per tick. */
function setup(windowMs: number) {
  let now = 1_760_000_000_000;
  const r = renderWithProviders(<Probe windowMs={windowMs} />, {
    transportOptions: { now: () => now, historyRows: 3600 },
    backfillMs: HOUR,
  });
  const tick = () => {
    now += 1000;
    act(() => r.transport.tick());
  };
  return { ...r, tick };
}

function state() {
  if (!storeRef) throw new Error("no store");
  return storeRef.getState();
}

describe("useWindowSeries", () => {
  it("reads the raw interval grid for a short window", async () => {
    const { tick } = setup(60_000);
    await screen.findByText("points 60");
    tick();
    for (const k of KEYS) {
      expect(latest?.values[k]).toEqual(
        seriesWindow(state(), k, 60_000).values
      );
    }
    expect(latest?.tEndMs).toBe(state().lastTsMs);
  });

  it("buckets a 1 h window, matching a full recompute tick after tick", async () => {
    const { tick } = setup(HOUR);
    await screen.findByText("points 1200");
    // Three-second buckets: ticks inside one bucket and across a close.
    for (let t = 0; t < 7; t++) {
      tick();
      const s = state();
      const last = bucketIndex(s.lastTsMs ?? 0, 3000);
      const full = bucketAverages(s, KEYS, 3000, last - 1199, last);
      expect(latest?.intervalMs).toBe(3000);
      expect(latest?.tEndMs).toBe(last * 3000);
      for (const k of KEYS) expect(latest?.values[k]).toEqual(full[k]);
    }
  });

  /**
   * One `backfill` message carrying many rows (a resume sent unchunked): the
   * 1 h chart and the heatmap buckets must equal a full recompute and have
   * no empty bucket over the appended span. `back` starts the rows that far
   * before the newest held row on a new timeline (a clock step, so the
   * reducer truncates first); `endPhase` lands the newest row that far into
   * a 3 s bucket, so the append closes exactly at a boundary or just after.
   */
  describe.each([
    { name: "a 30-minute resume", rows: 1800, back: 0, endPhase: null },
    {
      // The two minutes held plus these overrun the ring.
      name: "an append that wraps the ring",
      rows: RING_MAX_ROWS - 100,
      back: 0,
      endPhase: null,
      wraps: true,
    },
    {
      name: "an append of a whole ring",
      rows: RING_MAX_ROWS,
      back: 0,
      endPhase: null,
    },
    {
      name: "an append longer than the ring",
      rows: 9000,
      back: 0,
      endPhase: null,
    },
    {
      name: "an append ending on a bucket's first row",
      rows: 1800,
      back: 0,
      endPhase: 0,
    },
    {
      name: "an append ending on a bucket's last row",
      rows: 1800,
      back: 0,
      endPhase: 2000,
    },
    {
      name: "a truncate then append",
      rows: 1800,
      back: 600_000,
      endPhase: null,
    },
    {
      // The newest bucket stays put: only the epoch invalidates the cache.
      name: "a truncate then append up to the same newest row",
      rows: 1800,
      back: 1_800_000,
      endPhase: null,
    },
  ])("$name in one message", ({ rows, back, endPhase, wraps }) => {
    it("leaves no hole and matches a full recompute", async () => {
      const { transport } = setup(HOUR);
      await screen.findByText("points 1200");
      const s0 = state();
      const layoutNo = s0.layoutNo ?? 0;
      const width = s0.layouts[layoutNo]?.keys.length ?? 0;
      if (wraps) {
        const room = s0.columns.capacity - s0.columns.length;
        expect(rows).toBeGreaterThan(room);
        expect(rows).toBeLessThan(s0.columns.capacity);
      }
      let from = (s0.lastTsMs ?? 0) + 1000 - back;
      if (endPhase !== null) {
        const end = from + (rows - 1) * 1000;
        from += (endPhase - (end % 3000) + 3000) % 3000;
      }
      const values = Array.from({ length: rows }, () =>
        new Array<number | null>(width).fill(42)
      );
      act(() =>
        transport.push({
          kind: "backfill",
          holds_ms: [],
          layout_no: layoutNo,
          timeline: (s0.timeline ?? 0) + (back > 0 ? 1 : 0),
          start_ms: from,
          interval_ms: 1000,
          rows: values,
        })
      );
      const s = state();
      const newest = from + (rows - 1) * 1000;
      expect(s.lastTsMs).toBe(newest);
      if (back > 0) expect(s.rowsEpoch).toBeGreaterThan(s0.rowsEpoch);
      if (endPhase !== null) expect(newest % 3000).toBe(endPhase);

      const last = bucketIndex(newest, 3000);
      const full = bucketAverages(s, KEYS, 3000, last - 1199, last);
      const firstNew = Math.max(0, bucketIndex(from, 3000) + 1 - (last - 1199));
      const heatLast = bucketIndex(newest, HEAT_MS);
      const heatFull = bucketAverages(
        s,
        KEYS,
        HEAT_MS,
        heatLast - HEAT_COUNT + 1,
        heatLast
      );
      const heatFirstNew = Math.max(
        0,
        bucketIndex(from, HEAT_MS) + 1 - (heatLast - HEAT_COUNT + 1)
      );
      for (const k of KEYS) {
        expect(latest?.values[k]).toEqual(full[k]);
        expect(latest?.values[k]?.slice(firstNew)).not.toContain(null);
        expect(heat?.values[k]).toEqual(heatFull[k]);
        expect(heat?.values[k]?.slice(heatFirstNew)).not.toContain(null);
      }
    });
  });
});

/**
 * A `mean` and a gauge sampled every 4 s or 5 s (held half a second more) on
 * a 1 s grid: the first sample after a bucket boundary fills its span back
 * into the bucket that just closed (at 5 s and 2 s buckets, into the one
 * before it too), so the buckets must equal a fresh recompute every tick,
 * not the value memoized when the bucket closed.
 */
describe("a slow series crossing a bucket boundary", () => {
  const SLOW = ["slow.mean", "slow.gauge"];
  let slow: WindowSeries | null = null;
  let slowHeat: ReturnType<typeof useRingBuckets> | null = null;

  function SlowProbe() {
    storeRef = useHostStore();
    slow = useWindowSeries(SLOW, HOUR);
    slowHeat = useRingBuckets(SLOW, 2000, 30);
    return <p>slow {slow.values["slow.mean"]?.length}</p>;
  }

  it.each([4, 5])(
    "recomputes the closed buckets a %i s sample fills back into",
    async (every) => {
      const r = renderWithProviders(<SlowProbe />, {
        transportOptions: { now: () => 1_760_000_000_000, historyRows: 3600 },
        backfillMs: HOUR,
      });
      await screen.findByText("slow 1200");
      const s0 = state();
      const timeline = s0.timeline ?? 0;
      act(() => {
        r.transport.push({
          kind: "layout",
          layout_no: 99,
          kinds: ["mean", "gauge"],
          series: [seriesKey("slow.mean"), seriesKey("slow.gauge")],
        });
        r.transport.push({
          kind: "holds",
          layout_no: 99,
          holds_ms: [every * 1000 + 500, every * 1000 + 500],
        });
      });
      const t0 = s0.lastTsMs ?? 0;
      for (let i = 1; i <= 30; i++) {
        const v = i % every === 0 ? i : null;
        act(() =>
          r.transport.push({
            kind: "frame",
            layout_no: 99,
            timeline,
            ts_ms: t0 + i * 1000,
            values: [v, v],
            held: [v, v],
          })
        );
        const s = state();
        const last = bucketIndex(s.lastTsMs ?? 0, 3000);
        const full = bucketAverages(s, SLOW, 3000, last - 1199, last);
        const heatLast = bucketIndex(s.lastTsMs ?? 0, 2000);
        const heatFull = bucketAverages(s, SLOW, 2000, heatLast - 29, heatLast);
        for (const k of SLOW) {
          expect(slow?.values[k]).toEqual(full[k]);
          expect(slowHeat?.values[k]).toEqual(heatFull[k]);
        }
      }
    }
  );
});
