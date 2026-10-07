import type { ChartWindow, LiveMsg } from "@core/generated/bindings";
import { MOCK_HOST_ID } from "./mock/fixtures";
import { createMockTransport } from "./mock-transport";

const NOW = 1_800_000_000_000;

async function subscribe(
  scenarios?: Parameters<typeof createMockTransport>[0]
) {
  const t = createMockTransport({ now: () => NOW, ...scenarios });
  const msgs: LiveMsg[] = [];
  const sub = await t.subscribeLive(MOCK_HOST_ID, (m) => msgs.push(m));
  return { t, msgs, sub };
}

describe("mock transport", () => {
  it("sends caps, status, layout, then backfill (D-049)", async () => {
    const { msgs, sub } = await subscribe();
    expect(msgs.map((m) => m.kind)).toEqual([
      "caps",
      "status",
      "layout",
      "backfill",
    ]);
    expect(sub.info.status).toBe("ok");
    const backfill = msgs[3];
    expect(backfill?.kind === "backfill" && backfill.rows.length).toBe(60);
  });

  it("splits the backfill around the sleep-gap hole", async () => {
    const { msgs } = await subscribe({ scenarios: ["sleep-gap"] });
    const segments = msgs.filter((m) => m.kind === "backfill");
    expect(segments).toHaveLength(2);
  });

  it("ticks frames with values and held in layout order", async () => {
    const { t, msgs } = await subscribe();
    const layout = msgs.find((m) => m.kind === "layout");
    t.tick();
    const frame = msgs[msgs.length - 1];
    expect(frame?.kind).toBe("frame");
    if (frame?.kind !== "frame" || layout?.kind !== "layout") return;
    expect(frame.values).toHaveLength(layout.series.length);
    expect(frame.held).toHaveLength(layout.series.length);
  });

  it("sends no frames while paused or stale", async () => {
    for (const scenario of ["paused", "stale"] as const) {
      const { t, msgs } = await subscribe({ scenarios: [scenario] });
      const before = msgs.length;
      t.tick();
      expect(msgs.length).toBe(before);
    }
  });

  it("reports scenario capabilities", async () => {
    const t = createMockTransport({
      scenarios: ["no-battery", "unknown-chip"],
    });
    const caps = await t.getCapabilities(MOCK_HOST_ID);
    expect(caps.status).toBe("ok");
    if (caps.status !== "ok") return;
    expect(caps.data.modules.battery).toBe("not_present");
    expect(caps.data.modules.sensors).toEqual({ unsupported: "unknown_chip" });
  });

  it("bumps the settings revision and emits settings-changed", async () => {
    const t = createMockTransport();
    const seen: number[] = [];
    t.onSettingsChanged((e) => seen.push(e.revision));
    const before = await t.getSettings();
    const res = await t.updateSettings({ sampling: { interval_ms: 2000 } });
    expect(res.status).toBe("ok");
    expect(seen).toEqual([before.revision + 1]);
  });

  it("rejects an interval outside the allowed set", async () => {
    const t = createMockTransport();
    const res = await t.updateSettings({ sampling: { interval_ms: 1234 } });
    expect(res).toMatchObject({
      status: "error",
      error: { kind: "invalid_settings" },
    });
  });

  it("accepts the 30 s interval and re-times the auto-tick to it", async () => {
    vi.useFakeTimers();
    try {
      const t = createMockTransport({ autoTick: true });
      const msgs: LiveMsg[] = [];
      await t.subscribeLive(MOCK_HOST_ID, (m) => msgs.push(m));
      const frames = () => msgs.filter((m) => m.kind === "frame").length;
      vi.advanceTimersByTime(3000);
      expect(frames()).toBe(3);

      const res = await t.updateSettings({ sampling: { interval_ms: 30_000 } });
      expect(res.status).toBe("ok");
      expect(msgs.at(-1)).toMatchObject({
        kind: "status",
        interval_ms: 30_000,
      });
      vi.advanceTimersByTime(29_000);
      expect(frames()).toBe(3);
      vi.advanceTimersByTime(1000);
      expect(frames()).toBe(4);
      t.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it("starts at a given interval with backfill rows that far apart", async () => {
    const { msgs } = await subscribe({ intervalMs: 30_000, historyRows: 120 });
    expect(msgs[1]).toMatchObject({ kind: "status", interval_ms: 30_000 });
    const backfill = msgs.find((m) => m.kind === "backfill");
    expect(backfill?.kind === "backfill" && backfill.interval_ms).toBe(30_000);
    // The default 60 s backfill holds two 30 s rows.
    expect(backfill?.kind === "backfill" && backfill.rows.length).toBe(2);
  });

  it("validates the history size limit", async () => {
    const t = createMockTransport();
    const ok = await t.updateSettings({ history: { size_limit_mb: 500 } });
    expect(ok.status === "ok" && ok.data.settings.history.size_limit_mb).toBe(
      500
    );
    const bad = await t.updateSettings({ history: { size_limit_mb: 200 } });
    expect(bad).toMatchObject({
      status: "error",
      error: { kind: "invalid_settings" },
    });
  });

  it("starts at the 15m chart window, or the one given", async () => {
    const plain = await createMockTransport().getSettings();
    expect(plain.settings.general.chart_window).toBe("15m");
    const t = createMockTransport({ chartWindow: "1h" });
    expect((await t.getSettings()).settings.general.chart_window).toBe("1h");
  });

  it("saves a chart window and rejects one outside the set", async () => {
    const t = createMockTransport();
    const ok = await t.updateSettings({ general: { chart_window: "30m" } });
    expect(ok.status === "ok" && ok.data.settings.general.chart_window).toBe(
      "30m"
    );
    const bad = await t.updateSettings({
      general: { chart_window: "2h" as ChartWindow },
    });
    expect(bad).toMatchObject({
      status: "error",
      error: { kind: "invalid_settings" },
    });
    expect((await t.getSettings()).settings.general.chart_window).toBe("30m");
  });

  it("fails every settings write with settingsNotSaved", async () => {
    const t = createMockTransport({ settingsNotSaved: true });
    const before = await t.getSettings();
    const res = await t.updateSettings({ general: { chart_window: "30m" } });
    expect(res).toMatchObject({
      status: "error",
      error: { kind: "settings_not_saved" },
    });
    expect(await t.getSettings()).toEqual(before);
  });

  it("reports history health and emits its changes", async () => {
    const t = createMockTransport({
      now: () => NOW,
      scenarios: ["history-trimmed"],
    });
    const health = await t.historyHealth(MOCK_HOST_ID);
    expect(health).toMatchObject({
      status: "ok",
      data: { low_disk_paused: false, trimmed_limit_bytes: 150_000_000 },
    });
    const seen: boolean[] = [];
    t.onHistoryHealthChanged((e) =>
      seen.push(e.health.trimmed_before_ms === null)
    );
    await t.clearHistory(MOCK_HOST_ID);
    expect(seen).toEqual([true]);

    const down = createMockTransport({ scenarios: ["history-unavailable"] });
    expect(await down.historyHealth(MOCK_HOST_ID)).toMatchObject({
      status: "error",
      error: { kind: "history_unavailable" },
    });
  });

  it("errors on an unknown host", async () => {
    const t = createMockTransport();
    const res = await t.getHost("nope");
    expect(res).toMatchObject({
      status: "error",
      error: { kind: "unknown_host" },
    });
  });

  it("stops delivering after unsubscribe", async () => {
    const { t, msgs, sub } = await subscribe();
    sub.unsubscribe();
    const before = msgs.length;
    t.tick();
    expect(msgs.length).toBe(before);
    expect(t.subscriberCount()).toBe(0);
  });
});
