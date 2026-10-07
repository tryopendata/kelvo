/**
 * The mock's D-066 live channel and the commands added with it, so the
 * frontend tests and the dev server exercise what Rust sends.
 */
import type { LiveMsg, LiveProcess } from "@core/generated/bindings";
import { initialHostLive, reduceLive } from "./live-state";
import {
  MOCK_HOST_ID,
  type ScenarioName,
  scenarioFlags,
} from "./mock/fixtures";
import { MockGenerator } from "./mock/generator";
import {
  createMockTransport,
  EARLIER_CHUNK_ROWS,
  RECENT_MS,
} from "./mock-transport";
import { readFailure } from "./read-failure";
import type { LiveOptions } from "./transport";

const NOW = 1_800_000_000_000;

async function subscribe(
  options: Parameters<typeof createMockTransport>[0] = {},
  live: LiveOptions = {}
) {
  const t = createMockTransport({ now: () => NOW, ...options });
  const msgs: LiveMsg[] = [];
  const sub = await t.subscribeLive(MOCK_HOST_ID, (m) => msgs.push(m), live);
  return { t, msgs, sub };
}

const ofKind = <K extends LiveMsg["kind"]>(msgs: LiveMsg[], kind: K) =>
  msgs.filter((m): m is Extract<LiveMsg, { kind: K }> => m.kind === kind);

describe("mock live channel (D-066)", () => {
  afterEach(() => vi.useRealTimers());

  it("projects layouts and frames to the named series", async () => {
    const { t, msgs } = await subscribe(
      {},
      {
        series: [
          { metric: "cpu.total", labels: [] },
          { metric: "cpu.load", labels: [["core", "P0"]] },
        ],
      }
    );
    const [layout] = ofKind(msgs, "layout");
    expect(layout?.series).toEqual([
      { metric: "cpu.total", labels: [] },
      { metric: "cpu.load", labels: [["core", "P0"]] },
    ]);
    expect(ofKind(msgs, "backfill")[0]?.rows[0]).toHaveLength(2);
    t.tick();
    const [frame] = ofKind(msgs, "frame");
    expect(frame?.values).toHaveLength(2);
    expect(frame?.held).toHaveLength(2);
  });

  it("sends two minutes before returning and the rest as earlier chunks after the first frame", async () => {
    vi.useFakeTimers();
    const { t, msgs, sub } = await subscribe(
      { historyRows: 3600 },
      { backfillMs: 3_600_000 }
    );
    const recent = ofKind(msgs, "backfill");
    expect(recent.reduce((n, m) => n + m.rows.length, 0)).toBe(
      RECENT_MS / 1000
    );
    expect(sub.info).toMatchObject({
      status: "ok",
      data: { backfill_rows: 120, earlier_rows: 3480 },
    });
    expect(ofKind(msgs, "backfill_earlier")).toEqual([]);

    t.tick();
    // One chunk per task, as a Tauri Channel delivers them.
    await vi.advanceTimersByTimeAsync(0);
    expect(ofKind(msgs, "backfill_earlier")).toHaveLength(1);
    await vi.runAllTimersAsync();
    const chunks = ofKind(msgs, "backfill_earlier");
    // History is tray-only until its last three minutes, and a segment ends
    // where holds change (D-090): 60 visible rows, then 3420 tray-only ones.
    expect(chunks).toHaveLength(1 + Math.ceil(3420 / EARLIER_CHUNK_ROWS));
    for (const c of chunks) expect(c.rows.length).toBeLessThanOrEqual(600);
    // Newest first, each older than everything before it.
    let floor = recent[0]?.start_ms ?? 0;
    for (const c of chunks) {
      expect(c.start_ms + (c.rows.length - 1) * 1000).toBeLessThan(floor);
      floor = c.start_ms;
    }
    expect(sub.info.status === "ok" && sub.info.data.earlier_start_ms).toBe(
      floor
    );
    t.dispose();
  });

  it("starts the earlier chunks after 1 s when no frame comes", async () => {
    vi.useFakeTimers();
    const { t, msgs } = await subscribe(
      { historyRows: 600, scenarios: ["paused"] },
      { backfillMs: 3_600_000 }
    );
    await vi.advanceTimersByTimeAsync(999);
    expect(ofKind(msgs, "backfill_earlier")).toEqual([]);
    await vi.runAllTimersAsync();
    expect(ofKind(msgs, "backfill_earlier").length).toBeGreaterThan(0);
    t.dispose();
  });

  it("steps the clock back with a new layout and an older frame", async () => {
    let now = NOW;
    const t = createMockTransport({ now: () => now });
    const msgs: LiveMsg[] = [];
    await t.subscribeLive(MOCK_HOST_ID, (m) => msgs.push(m));
    now += 1000;
    t.tick();
    t.stepClock(-300_000);
    now += 1000;
    t.tick();
    const layouts = ofKind(msgs, "layout");
    const frames = ofKind(msgs, "frame");
    expect(layouts.map((l) => l.layout_no)).toEqual([1, 2]);
    expect(frames.map((f) => f.layout_no)).toEqual([1, 2]);
    expect(frames.map((f) => f.timeline)).toEqual([0, 1]);
    expect(frames[1]?.ts_ms).toBeLessThan(frames[0]?.ts_ms ?? 0);
  });

  it("resumes a shown window with the span it missed, 600 rows per task, before the next frame", async () => {
    vi.useFakeTimers();
    let now = NOW;
    const t = createMockTransport({ now: () => now, historyRows: 60 });
    const msgs: LiveMsg[] = [];
    await t.subscribeLive(MOCK_HOST_ID, (m) => msgs.push(m), {
      backfillMs: 3_600_000,
    });
    msgs.length = 0;
    t.setWindowVisible(false);
    now += 1000;
    t.tick();
    t.fastForward(30 * 60_000);
    expect(msgs).toEqual([]);
    t.setWindowVisible(true);
    now += 1000;
    t.tick();
    // Nothing yet: the resume goes one message per task, the frame after it.
    expect(msgs).toEqual([]);
    await vi.advanceTimersByTimeAsync(0);
    expect(msgs).toHaveLength(1);
    await vi.runAllTimersAsync();
    // Hidden, the engine ticked at the background's 2 s (D-094): 901 rows for
    // the half hour, and the window never heard about the slower tick.
    const backfills = ofKind(msgs, "backfill");
    expect(backfills.map((m) => m.rows.length)).toEqual([600, 301]);
    expect(backfills.map((m) => m.interval_ms)).toEqual([2000, 2000]);
    for (let i = 1; i < backfills.length; i++) {
      const prev = backfills[i - 1] as (typeof backfills)[number];
      expect(backfills[i]?.start_ms).toBe(
        prev.start_ms + prev.rows.length * 2000
      );
    }
    expect(ofKind(msgs, "status")).toEqual([]);
    expect(msgs[msgs.length - 1]?.kind).toBe("frame");
    t.dispose();
  });

  it("reports display sleep in the status and sends no frames during it", async () => {
    const { t, msgs } = await subscribe({}, { minPeriodMs: 5000 });
    expect(ofKind(msgs, "status")[0]).toMatchObject({
      display_idle: false,
      frame_period_ms: 5000,
    });
    t.setDisplayIdle(true);
    t.tick();
    expect(ofKind(msgs, "frame")).toEqual([]);
    expect(ofKind(msgs, "status").at(-1)?.display_idle).toBe(true);
    t.dispose();
  });

  it("paces frames to 2 s in Performance mode on AC", async () => {
    let now = NOW;
    const t = createMockTransport({
      now: () => now,
      scenarios: ["no-battery"],
    });
    const msgs: LiveMsg[] = [];
    const appearance: string[] = [];
    t.onWindowAppearanceChanged((e) =>
      appearance.push(e.appearance.performance)
    );
    await t.subscribeLive(MOCK_HOST_ID, (m) => msgs.push(m));
    await t.updateSettings({ sampling: { performance_mode: true } });
    expect(ofKind(msgs, "status").at(-1)).toMatchObject({
      interval_ms: 1000,
      performance: "setting",
      frame_period_ms: 2000,
    });
    expect(appearance).toEqual(["setting"]);
    for (let i = 0; i < 4; i++) {
      now += 1000;
      t.tick();
    }
    expect(ofKind(msgs, "frame").map((f) => f.ts_ms - NOW)).toEqual([
      1000, 3000,
    ]);
    await t.updateSettings({ sampling: { performance_mode: false } });
    expect(ofKind(msgs, "status").at(-1)).toMatchObject({
      performance: "off",
      frame_period_ms: 1000,
    });
    expect(appearance).toEqual(["setting", "off"]);
    t.dispose();
  });

  it("doubles the tick in Low Power Mode, as the engine does", async () => {
    const { t, msgs } = await subscribe({ scenarios: ["no-battery"] });
    t.setLowPowerMode(true);
    expect(ofKind(msgs, "status").at(-1)).toMatchObject({
      interval_ms: 2000,
      frame_period_ms: 2000,
      performance: "low_power_mode",
    });
    t.setLowPowerMode(false);
    expect(ofKind(msgs, "status").at(-1)).toMatchObject({
      interval_ms: 1000,
      performance: "off",
    });
    t.dispose();
  });

  it("prefers the setting to Low Power Mode as the reason", async () => {
    const { t, msgs } = await subscribe({ scenarios: ["low-power-mode"] });
    expect(ofKind(msgs, "status")[0]?.performance).toBe("low_power_mode");
    await t.updateSettings({ sampling: { performance_mode: true } });
    expect(ofKind(msgs, "status").at(-1)?.performance).toBe("setting");
    t.setLowPowerMode(false);
    expect(ofKind(msgs, "status").at(-1)?.performance).toBe("setting");
    expect((await t.getWindowAppearance()).performance).toBe("setting");
    t.dispose();
  });

  it("shapes and paces process rows by the window's view and stream", async () => {
    let now = NOW;
    const t = createMockTransport({ now: () => now });
    const batches: number[] = [];
    const sub = await t.subscribeLive(MOCK_HOST_ID, (m) => {
      if (m.kind === "processes") batches.push(m.rows.length);
    });
    const stream = sub.info.status === "ok" ? sub.info.data.stream : -1;
    await t.setProcessInterest(
      MOCK_HOST_ID,
      true,
      { limit: 5, sort: ["cpu", "memory"], period_ms: 5000 },
      stream
    );
    await Promise.resolve();
    for (let i = 0; i < 6; i++) {
      now += 1000;
      t.tick();
    }
    // The first batch at once, the next one 5 s later; at most 5 per key.
    expect(batches).toHaveLength(2);
    for (const n of batches) expect(n).toBeLessThanOrEqual(10);

    // Interest tagged with another stream does not count.
    await t.setProcessInterest(MOCK_HOST_ID, true, null, stream + 100);
    expect(t.processInterest()).toBeNull();
  });
});

describe("mock per-process network (D-081)", () => {
  async function firstBatch(scenarios: ScenarioName[], network: boolean) {
    const t = createMockTransport({ now: () => NOW, scenarios });
    let rows: LiveProcess[] = [];
    const sub = await t.subscribeLive(MOCK_HOST_ID, (m) => {
      if (m.kind === "processes") rows = m.rows;
    });
    const stream = sub.info.status === "ok" ? sub.info.data.stream : -1;
    await t.setProcessInterest(
      MOCK_HOST_ID,
      true,
      { limit: 3, sort: ["net_total"], period_ms: null, network },
      stream
    );
    await Promise.resolve();
    t.dispose();
    return rows;
  }

  it("ranks by network rate when the view asks for rates", async () => {
    const rows = await firstBatch(["default"], true);
    expect(rows.map((p) => [p.name, p.net_rx_bps, p.net_tx_bps])).toEqual([
      ["Safari", 21.4e6, 0.7e6],
      ["com.docker.backend", 8.8e6, 0.8e6],
      ["node", 3.0e6, 1.2e6],
    ]);
  });

  it("sends no rates without the view flag or the capability", async () => {
    for (const rows of [
      await firstBatch(["default"], false),
      await firstBatch(["no-process-network"], true),
      await firstBatch(["appstore"], true),
    ]) {
      expect(rows).toHaveLength(3);
      expect(rows.every((p) => p.net_rx_bps === null)).toBe(true);
    }
  });
});

describe("mock per-process GPU (D-085)", () => {
  async function batches(scenarios: ScenarioName[], gpu: boolean) {
    const t = createMockTransport({ now: () => NOW, scenarios });
    const seen: LiveProcess[][] = [];
    const sub = await t.subscribeLive(MOCK_HOST_ID, (m) => {
      if (m.kind === "processes") seen.push(m.rows);
    });
    const stream = sub.info.status === "ok" ? sub.info.data.stream : -1;
    const view = { limit: 3, sort: ["gpu" as const], period_ms: null };
    await t.setProcessInterest(MOCK_HOST_ID, true, view, stream);
    await Promise.resolve();
    // Turning the flag on answers at once, without a tick.
    await t.setProcessInterest(MOCK_HOST_ID, true, { ...view, gpu }, stream);
    await Promise.resolve();
    t.dispose();
    return seen;
  }

  it("ranks by GPU time as soon as the view asks for it", async () => {
    const seen = await batches(["default"], true);
    expect(seen).toHaveLength(2);
    expect(seen[1]?.map((p) => [p.name, p.gpu_pct])).toEqual([
      ["WindowServer", 14.2],
      ["Figma", 9.8],
      ["Safari", 5.1],
    ]);
  });

  it("sends no GPU time without the view flag or the capability", async () => {
    for (const seen of [
      await batches(["default"], false),
      await batches(["no-process-gpu"], true),
      await batches(["appstore"], true),
    ]) {
      const rows = seen.flat();
      expect(rows.length).toBeGreaterThan(0);
      expect(rows.every((p) => p.gpu_pct === null)).toBe(true);
    }
  });
});

describe("mock history reset and edition", () => {
  it("resets a corrupt history, after which history answers", async () => {
    const t = createMockTransport({ scenarios: ["history-corrupt"] });
    const before = await t.historyHealth(MOCK_HOST_ID);
    expect(before).toMatchObject({
      status: "error",
      error: { kind: "history_unavailable", reason: { kind: "corrupt" } },
    });
    const health: unknown[] = [];
    t.onHistoryHealthChanged((e) => health.push(e.health));
    expect((await t.resetHistory()).status).toBe("ok");
    expect((await t.historyHealth(MOCK_HOST_ID)).status).toBe("ok");
    expect(health).toHaveLength(1);
  });

  it("answers store_busy while another process holds the file", async () => {
    const t = createMockTransport({ scenarios: ["history-locked"] });
    expect(await t.resetHistory()).toMatchObject({
      status: "error",
      error: { kind: "store_busy" },
    });
  });

  it("reports the App Store edition and refuses to signal there", async () => {
    const t = createMockTransport({ scenarios: ["appstore"] });
    expect(await t.getEdition()).toEqual({ process_signal: false });
    expect(await t.processSignal(MOCK_HOST_ID, 1, 1, "quit")).toEqual({
      status: "error",
      error: { kind: "unavailable" },
    });
    const full = createMockTransport();
    expect(await full.getEdition()).toEqual({ process_signal: true });
  });

  it("emits hosts-changed with the host list", () => {
    const t = createMockTransport();
    const seen: string[][] = [];
    t.onHostsChanged((e) => seen.push(e.hosts.map((h) => h.id)));
    t.emitHostsChanged();
    expect(seen).toEqual([[MOCK_HOST_ID]]);
  });
});

describe("mock totals and held staleness (D-092)", () => {
  const NET: LiveOptions["series"] = [
    { metric: "net.rx", labels: [["iface", "en0"]] },
    { metric: "net.rx_total", labels: [] },
    { metric: "net.tx_total", labels: [] },
  ];

  it("sums the reported interfaces, a gap when any one is", async () => {
    const { t, msgs } = await subscribe({}, { series: NET });
    t.tick();
    const [en0, rx, tx] = ofKind(msgs, "frame").at(-1)?.values ?? [];
    expect(rx).toBeCloseTo(en0 ?? 0, 2);
    expect(tx).not.toBeNull();

    t.setReadFailing("net.rx{iface=en0}", true);
    t.tick();
    const failed = ofKind(msgs, "frame").at(-1)?.values ?? [];
    expect(failed[0]).toBeNull();
    expect(failed[1]).toBeNull();
    // rx and tx are gated separately.
    expect(failed[2]).not.toBeNull();
  });

  it("sums two interfaces, and is a gap when one fails while the other reads", () => {
    const gen = new MockGenerator(scenarioFlags(["vpn"]));
    const at = (key: string) => gen.indexOf(key);
    const [en0, en7, total] = [
      at("net.rx{iface=en0}"),
      at("net.rx{iface=en7}"),
      at("net.rx_total"),
    ];
    expect(Math.min(en0, en7, total)).toBeGreaterThanOrEqual(0);
    const ok = gen.next(true).values;
    expect(ok[total]).toBeCloseTo((ok[en0] ?? 0) + (ok[en7] ?? 0), 2);

    gen.failing.add(en7);
    const failed = gen.next(true).values;
    expect(failed[en0]).not.toBeNull();
    expect(failed[en7]).toBeNull();
    expect(failed[total]).toBeNull();
  });

  it("nulls held past the hold, so readFailure surfaces", async () => {
    const { t, msgs } = await subscribe(
      {},
      { series: [{ metric: "cpu.total", labels: [] }] }
    );
    const live = () => msgs.reduce(reduceLive, initialHostLive(MOCK_HOST_ID));
    t.setReadFailing("cpu.total", true);
    // A 1 s series holds 2.5 s (HOLD_FACTOR): two missed ticks still hold.
    t.tick();
    t.tick();
    expect(ofKind(msgs, "frame").at(-1)?.held).toEqual([expect.any(Number)]);
    expect(readFailure(live(), "cpu.total")).toBeNull();

    t.tick();
    expect(ofKind(msgs, "frame").at(-1)?.held).toEqual([null]);
    expect(readFailure(live(), "cpu.total")).toMatchObject({
      lastGoodMs: expect.any(Number),
    });

    t.setReadFailing("cpu.total", false);
    t.tick();
    expect(readFailure(live(), "cpu.total")).toBeNull();
  });

  it("reports the power source and primary interface in the status", async () => {
    const { msgs } = await subscribe();
    expect(ofKind(msgs, "status")[0]).toMatchObject({
      power_source: "battery",
      primary_iface: "en0",
    });
    const desk = await subscribe({ scenarios: ["no-battery"] });
    expect(ofKind(desk.msgs, "status")[0]?.power_source).toBe("adapter");
    const vpn = await subscribe({ scenarios: ["vpn"] });
    expect(ofKind(vpn.msgs, "status")[0]?.primary_iface).toBeNull();
  });
});
