import type { LiveMsg, NetworkByApp } from "@core/generated/bindings";
import { createMockTransport } from "../mock-transport";
import { MOCK_HOST_ID } from "./fixtures";
import {
  appsReportedTo,
  burstWindows,
  NET_BURSTS,
  NET_COLLECTION_DELAY_MS,
  splitDirection,
} from "./net-apps";

const NOW = 1_800_000_000_000;
const HOUR = 3_600_000;

async function byApp(
  t: ReturnType<typeof createMockTransport>,
  from: number,
  to: number
): Promise<NetworkByApp> {
  const r = await t.queryNetworkByApp(MOCK_HOST_ID, from, to);
  if (r.status !== "ok") throw new Error(JSON.stringify(r.error));
  return r.data;
}

function sums(n: NetworkByApp) {
  const apps = (d: "rx" | "tx") =>
    n.apps.reduce((s, a) => s + a[`${d}_bytes`], 0) +
    n[`other_apps_${d}_bytes`];
  return {
    rx: apps("rx") + n.overhead_rx_bytes + n.system_rx_bytes - n.iface_rx_bytes,
    tx: apps("tx") + n.overhead_tx_bytes + n.system_tx_bytes - n.iface_tx_bytes,
  };
}

const docker = NET_BURSTS.find((b) => b.app === "Docker Desktop");

describe("mock query_network_by_app", () => {
  it("adds up to the interface bytes, with overhead and System never zero", async () => {
    const t = createMockTransport({ now: () => NOW });
    for (const span of [60_000, 300_000, 900_000]) {
      const n = await byApp(t, NOW - span, NOW);
      expect(sums(n)).toEqual({ rx: 0, tx: 0 });
      expect(n.clamped).toBe(false);
      expect(n.iface_rx_bytes).toBeGreaterThan(0);
      for (const v of [
        n.overhead_rx_bytes,
        n.overhead_tx_bytes,
        n.system_rx_bytes,
        n.system_tx_bytes,
        n.other_apps_rx_bytes,
      ]) {
        expect(v).toBeGreaterThan(0);
      }
      expect(n.apps.map((a) => a.name)).toContain("Google Chrome");
      expect(n.apps.some((a) => a.name === "")).toBe(false);
    }
  });

  it("matches the bytes the live chart's net.rx rows carry", async () => {
    const t = createMockTransport({ now: () => NOW });
    const msgs: LiveMsg[] = [];
    await t.subscribeLive(MOCK_HOST_ID, (m) => msgs.push(m), {
      backfillMs: 120_000,
      series: [{ metric: "net.rx", labels: [] }],
    });
    const rows = msgs.flatMap((m) =>
      m.kind === "backfill"
        ? m.rows.map((r, i) => ({
            ts: m.start_ms + i * m.interval_ms,
            v: r.reduce<number>((s, x) => s + (x ?? 0), 0),
          }))
        : []
    );
    // Whole buckets, so the request is not widened.
    const from = NOW - 100_000;
    const to = NOW - 20_000;
    const want = rows
      .filter((r) => r.ts >= from && r.ts < to)
      .reduce((s, r) => s + Math.round(r.v), 0);
    const n = await byApp(t, from, to);
    expect(n.iface_rx_bytes).toBe(want);
  });

  it("charges the Docker spike on the chart to Docker Desktop", async () => {
    const t = createMockTransport({ now: () => NOW });
    if (!docker) throw new Error("no Docker burst");
    const [start, end] = burstWindows(docker, NOW, NOW - 120_000, NOW)[0] ?? [
      0, 0,
    ];
    expect([start, end]).toEqual([NOW - 90_000, NOW - 60_000]);
    const spike = await byApp(t, start, end);
    expect(spike.apps[0]?.name).toBe("Docker Desktop");
    expect(spike.apps[0]?.rx_bytes).toBeGreaterThan(0.6 * spike.iface_rx_bytes);
    // Just after it, Docker Desktop is gone.
    const after = await byApp(t, NOW - 50_000, NOW - 10_000);
    expect(after.apps.map((a) => a.name)).not.toContain("Docker Desktop");
    expect(after.apps[0]?.name).toBe("Google Chrome");
    // And the chart shows it: the interface rate during the burst is over
    // twice the rate after it.
    const rate = (n: NetworkByApp) => n.iface_rx_bytes / n.measured_ms;
    expect(rate(spike)).toBeGreaterThan(2 * rate(after));
  });

  it("is deterministic for the same clock", async () => {
    const a = await byApp(
      createMockTransport({ now: () => NOW }),
      NOW - 300_000,
      NOW
    );
    const b = await byApp(
      createMockTransport({ now: () => NOW }),
      NOW - 300_000,
      NOW
    );
    expect(a).toEqual(b);
  });

  it("widens to whole buckets and reports partial and missing coverage", async () => {
    const t = createMockTransport({ now: () => NOW });
    // To just past the newest row, at NOW.
    const n = await byApp(t, NOW - HOUR + 3_000, NOW + 1);
    expect(n.from_ms % 10_000).toBe(0);
    expect(n.to_ms % 10_000).toBe(0);
    expect(n.coverage[0]?.tier).toBeNull();
    // The ring's 600 rows of 1 s, less the 40 s before per-app collection
    // and the rows the per-app stream has not reported yet.
    const pending = NOW - appsReportedTo(NOW) + 1000;
    expect(n.measured_ms).toBe(600_000 - NET_COLLECTION_DELAY_MS - pending);
    expect(n.measured_ms).toBeLessThan(n.to_ms - n.from_ms);
    expect(sums(n)).toEqual({ rx: 0, tx: 0 });
  });

  it("keeps the trailing buckets open until the per-app stream reports past them", async () => {
    // The newest row is 6 s past an edge; the per-app stream reports 4 s past.
    let clock = NOW + 6_000;
    const t = createMockTransport({ now: () => clock, autoTick: false });
    const open = await byApp(t, NOW - 10_000, NOW + 10_000);
    expect(open.complete_to_ms).toBe(NOW);
    // The open bucket has all of its interface bytes so far, 4 s of apps.
    const tail = await byApp(t, NOW, NOW + 10_000);
    expect(tail.measured_ms).toBe(4_000);
    expect(tail.complete_to_ms).toBe(NOW);
    const closed = await byApp(t, NOW - 10_000, NOW);
    expect(closed.complete_to_ms).toBe(closed.to_ms);
    // 2 s past the next edge the per-app stream has not reported past NOW + 10 s.
    const advance = (n: number) => {
      for (let i = 0; i < n; i++) {
        clock += 1000;
        t.tick();
      }
    };
    advance(6);
    expect((await byApp(t, NOW, NOW + 10_000)).complete_to_ms).toBe(NOW);
    advance(2);
    expect((await byApp(t, NOW, NOW + 10_000)).complete_to_ms).toBe(
      NOW + 10_000
    );
  });

  it("records nothing without NetworkStatistics or with Network history off", async () => {
    const appstore = createMockTransport({
      now: () => NOW,
      scenarios: ["appstore"],
    });
    const none = await byApp(appstore, NOW - 60_000, NOW);
    expect(none.measured_ms).toBe(0);
    expect(none.apps).toEqual([]);
    expect(none.coverage.every((s) => s.tier === null)).toBe(true);

    let clock = NOW;
    const t = createMockTransport({ now: () => clock });
    await t.updateSettings({ history: { network_history: false } });
    for (let i = 0; i < 20; i++) {
      clock += 1000;
      t.tick();
    }
    const off = await byApp(t, NOW, clock);
    expect(off.measured_ms).toBe(0);
    const before = await byApp(t, NOW - 60_000, NOW);
    expect(before.measured_ms).toBe(60_000);
  });

  it("answers unknown hosts and backwards ranges with typed errors", async () => {
    const t = createMockTransport({ now: () => NOW });
    const unknown = await t.queryNetworkByApp(
      "nope" as typeof MOCK_HOST_ID,
      0,
      1
    );
    expect(unknown.status === "error" && unknown.error.kind).toBe(
      "unknown_host"
    );
    const back = await t.queryNetworkByApp(MOCK_HOST_ID, NOW, NOW - 1);
    expect(back.status === "error" && back.error.kind).toBe("invalid_argument");
    const down = createMockTransport({
      now: () => NOW,
      scenarios: ["history-unavailable"],
    });
    // Live-only: the engine's ring still answers.
    const u = await down.queryNetworkByApp(MOCK_HOST_ID, NOW - 60_000, NOW);
    expect(u.status).toBe("ok");
    const long = await t.queryNetworkByApp(
      MOCK_HOST_ID,
      NOW - 91 * 86_400_000,
      NOW
    );
    expect(long.status === "error" && long.error.kind).toBe("invalid_argument");
  });
});

describe("splitDirection", () => {
  it("mirrors Rust: overhead shrinks first, clamp past the slack", () => {
    expect(splitDirection(1_000_000, 1_000, 800_000)).toEqual({
      overhead: 66_000,
      system: 134_000,
      clamped: false,
    });
    expect(splitDirection(1_000_000, 1_000, 970_000)).toEqual({
      overhead: 30_000,
      system: 0,
      clamped: false,
    });
    expect(splitDirection(1_000_000, 10, 1_900_000).clamped).toBe(true);
  });
});
