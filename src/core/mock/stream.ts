import { RING_SPAN_MS } from "@core/generated/bindings";
import type { MockChannels } from "./channels";
import type { MockCommands } from "./commands";
import type {
  HistoryRow,
  LiveSub,
  MockTransport,
  MockTransportOptions,
} from "./contracts";
import type { createMockDocker } from "./docker";
import { MOCK_HOST_ID } from "./fixtures";
import type { MockPower } from "./power";
import type { MockProcesses } from "./processes";
import {
  EARLIER_AFTER_MS,
  effectiveMinPeriodMs,
  framePeriodMs,
  layoutMsg,
  ok,
  pickFor,
  project,
} from "./protocol";
import type { MockRemotes } from "./remote";
import type { MockRing } from "./ring";
import { holdMs } from "./series";
export class MockStream {
  private ticks = 0;
  private nextStream = 1;
  private timer: ReturnType<typeof setInterval> | null = null;
  constructor(
    private readonly options: MockTransportOptions,
    private readonly ring: MockRing,
    private readonly power: MockPower,
    private readonly channels: MockChannels,
    private readonly processes: MockProcesses,
    private readonly remote: MockRemotes,
    private readonly commands: MockCommands,
    private readonly docker: ReturnType<typeof createMockDocker>
  ) {}
  readonly tick = () => {
    this.docker.tick();
    if (this.power.status.paused || this.ring.flags.stale) return;
    this.ticks++;
    if (this.ring.flags.clockStep && this.ticks === 11)
      this.stepClock(-300_000);
    if (this.ring.flags.hiddenResume && this.ticks === 11) {
      this.setWindowVisible(false);
      this.fastForward(30 * 60_000);
      // Shown in a later task, so anything the hidden window was sent
      // renders before it is shown, as it would sit in a real webview.
      setTimeout(() => {
        this.setWindowVisible(true);
        // The perf gate measures long tasks from here: building the messages
        // above is Rust's work, applying them is the webview's.
        performance.mark("kelvo:resume");
      }, 50);
    }
    const ts = this.ring.now() + this.ring.clockOffset;
    // Adaptive collectors idle unless a window is taking frames.
    this.ring.gen.trayOnly = !this.power.receiving();
    this.ring.gen.intervalMs = this.power.status.interval_ms;
    const { values, held, cadences } = this.ring.gen.next();
    this.ring.withBursts(ts, values);
    this.ring.withBursts(ts, held);
    this.ring.pushRow(ts, values, cadences, this.power.status.interval_ms);
    const holds = (this.ring.rows[this.ring.rows.length - 1] as HistoryRow)
      .holds;
    if (!this.power.receiving()) return;
    for (const sub of [...this.channels.live.values()]) {
      const period = effectiveMinPeriodMs(
        sub.minPeriodMs,
        this.power.status.performance
      );
      const due =
        period === null ||
        sub.lastFrameMs === null ||
        ts - sub.lastFrameMs >= period - this.power.status.interval_ms / 2;
      if (!due) continue;
      sub.lastFrameMs = ts;
      sub.lastSentMs = ts;
      sub.sentTimeline = this.ring.timeline;
      // A thinned channel's frames are a frame period apart: no hold is
      // shorter than that period's, as Rust sends it.
      const floor = holdMs(
        framePeriodMs(
          this.power.status.interval_ms,
          effectiveMinPeriodMs(sub.minPeriodMs, this.power.status.performance)
        )
      );
      const frameHolds = project(holds, sub.pick).map((h) =>
        Math.max(h, floor)
      );
      const joined = frameHolds.join(",");
      if (sub.holdsSent !== joined) {
        sub.holdsSent = joined;
        this.channels.deliver(sub, {
          kind: "holds",
          layout_no: this.ring.layoutNo,
          holds_ms: frameHolds,
        });
      }
      this.channels.deliver(sub, {
        kind: "frame",
        ts_ms: ts,
        layout_no: this.ring.layoutNo,
        timeline: this.ring.timeline,
        values: project(values, sub.pick),
        held: project(held, sub.pick),
      });
      this.channels.sendEarlier(sub);
    }
    this.processes.sendProcesses(ts);
  };
  readonly ensureTimer = () => {
    if (!this.options.autoTick || this.timer !== null) return;
    this.timer = setInterval(this.tick, this.power.status.interval_ms);
  };
  readonly retime = () => {
    if (this.timer === null) return;
    clearInterval(this.timer);
    this.timer = null;
    this.ensureTimer();
  };
  readonly subscribeLive: MockTransport["subscribeLive"] = async (
    host,
    onMsg,
    opts = {}
  ) => {
    const remote = this.remote.remotes.get(host);
    if (remote) {
      this.commands.record(
        "subscribe_live",
        host,
        opts.backfillMs ?? 60000,
        opts.series ?? null
      );
      return remote.peer.subscribeLive(MOCK_HOST_ID, onMsg, opts);
    }
    const backfillMs = Math.min(opts.backfillMs ?? 60_000, RING_SPAN_MS);
    this.commands.record(
      "subscribe_live",
      host,
      backfillMs,
      opts.series ?? null
    );
    const bad = this.commands.unknownHost(host);
    if (bad) return { info: bad, unsubscribe: () => {} };
    const pick = pickFor(this.ring.gen.keys, opts.series);
    const sub: LiveSub = {
      onMsg,
      stream: this.nextStream++,
      pick,
      series: project(this.ring.gen.keys, pick),
      minPeriodMs: opts.minPeriodMs ?? null,
      lastFrameMs: null,
      backfillMs,
      lastSentMs: null,
      sentTimeline: null,
      earlier: [],
      earlierStarted: false,
      timer: null,
      queue: [],
      queueTimer: null,
      holdsSent: null,
      statusSent: null,
    };
    this.channels.live.set(onMsg, sub);
    // A new subscription from the window ends interest tagged with another.
    this.processes.currentStream = sub.stream;
    const h = this.channels.history(sub, backfillMs);
    sub.earlier = h.earlier;
    onMsg({ kind: "caps", capabilities: this.ring.caps });
    // A hidden window gets its status on show, never the background's.
    if (this.power.windowVisible) this.channels.sendStatus(sub);
    onMsg(layoutMsg(this.ring.layoutNo, sub.series));
    for (const msg of h.recent) onMsg(msg);
    this.ensureTimer();
    // Rust starts the earlier chunks after the first frame, or after 1 s
    // when no frame comes (paused, stale).
    if (sub.earlier.length > 0) {
      sub.timer = setTimeout(
        () => this.channels.sendEarlier(sub),
        EARLIER_AFTER_MS
      );
    }
    const first = h.recent[0];
    return {
      info: ok({
        host,
        stream: sub.stream,
        backfill_start_ms: first?.kind === "backfill" ? first.start_ms : null,
        backfill_rows: h.recent.reduce(
          (n, m) => n + (m.kind === "backfill" ? m.rows.length : 0),
          0
        ),
        earlier_start_ms: h.earlierStart,
        earlier_rows: h.earlierRows,
      }),
      unsubscribe: () => {
        if (sub.timer !== null) clearTimeout(sub.timer);
        if (sub.queueTimer !== null) clearTimeout(sub.queueTimer);
        this.channels.live.delete(onMsg);
      },
    };
  };
  readonly stepClock: MockTransport["stepClock"] = (deltaMs) => {
    this.ring.clockOffset += deltaMs;
    this.ring.layoutNo++;
    this.ring.timeline++;
    for (const sub of this.channels.live.values()) {
      sub.earlier = [];
      if (sub.timer !== null) clearTimeout(sub.timer);
      sub.timer = null;
      sub.lastFrameMs = null;
      sub.holdsSent = null;
      sub.onMsg(layoutMsg(this.ring.layoutNo, sub.series));
    }
    this.processes.lastBatchMs = null;
  };
  readonly setWindowVisible: MockTransport["setWindowVisible"] = (visible) => {
    if (this.power.windowVisible === visible) return;
    const was = this.power.receiving();
    this.power.windowVisible = visible;
    this.docker.setVisible(this.power.receiving());
    // Rust's order (D-094): the tick changes before the stream resumes and
    // after it stops, so the window never gets the background's status.
    this.power.updatePower();
    if (!was && this.power.receiving()) this.channels.resume();
  };
  readonly setLowPowerMode: MockTransport["setLowPowerMode"] = (on) => {
    this.power.lowPowerMode = on;
    this.power.updatePower();
  };
  readonly setDisplayIdle: MockTransport["setDisplayIdle"] = (idle) => {
    if (this.power.status.display_idle === idle) return;
    const was = this.power.receiving();
    this.power.status = { ...this.power.status, display_idle: idle };
    this.docker.setVisible(this.power.receiving());
    this.channels.pushStatus();
    if (!was && this.power.receiving()) this.channels.resume();
  };
  readonly fastForward: MockTransport["fastForward"] = (ms) => {
    const interval = this.power.status.interval_ms;
    const last =
      this.ring.rows[this.ring.rows.length - 1]?.ts ??
      this.ring.now() + this.ring.clockOffset;
    this.ring.clockOffset += ms;
    // Time passes with the window hidden: only the tray is open.
    this.ring.gen.trayOnly = true;
    this.ring.gen.intervalMs = interval;
    for (
      let ts = last + interval;
      ts <= this.ring.now() + this.ring.clockOffset;
      ts += interval
    ) {
      const row = this.ring.gen.next();
      this.ring.pushRow(
        ts,
        this.ring.withBursts(ts, row.values),
        row.cadences,
        interval
      );
    }
    this.ring.gen.trayOnly = !this.power.receiving();
  };
  readonly setPaused: MockTransport["setPaused"] = async (paused) => {
    this.commands.record("set_paused", paused);
    if (this.power.status.paused === paused) return;
    this.power.status = { ...this.power.status, paused };
    this.channels.pushStatus();
  };
  dispose() {
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
    this.channels.dispose();
  }

  get methods() {
    return {
      subscribeLive: this.subscribeLive,
      stepClock: this.stepClock,
      setWindowVisible: this.setWindowVisible,
      setLowPowerMode: this.setLowPowerMode,
      setDisplayIdle: this.setDisplayIdle,
      fastForward: this.fastForward,
      setPaused: this.setPaused,
      tick: this.tick,
    };
  }
}
