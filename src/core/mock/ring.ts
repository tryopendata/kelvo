import type {
  HistoryRow,
  MockTransport,
  MockTransportOptions,
} from "./contracts";
import { capabilities, type ScenarioFlags } from "./fixtures";
import { MockGenerator } from "./generator";
import { mockGaps } from "./history";
import { burstsAt } from "./net-apps";
import { holdsOf } from "./protocol";
export class MockRing {
  readonly gen: MockGenerator;
  readonly caps: ReturnType<typeof capabilities>;
  readonly startMs: number;
  readonly ringStartMs: number;
  rows: HistoryRow[];
  clockOffset = 0;
  layoutNo = 1;
  timeline = 0;
  private en0Rx: number;
  private en0Tx: number;
  private rxTotal: number;
  private txTotal: number;
  readonly rxIdx: number[];
  readonly txIdx: number[];
  constructor(
    readonly flags: ScenarioFlags,
    readonly now: () => number,
    options: MockTransportOptions
  ) {
    this.gen = new MockGenerator(flags);
    this.startMs = now();
    const startMs = this.startMs;
    const gen = this.gen;
    const startInterval = options.intervalMs ?? 1000;
    gen.intervalMs = startInterval;
    const historyRows = options.historyRows ?? 600;
    const generated = gen.backfill(historyRows, Math.max(0, historyRows - 180));
    const en0 = (metric: string) =>
      gen.keys.findIndex(
        (k) =>
          k.metric === metric &&
          k.labels.some(([key, v]) => key === "iface" && v === "en0")
      );
    this.en0Rx = en0("net.rx");
    this.en0Tx = en0("net.tx");
    this.rxTotal = gen.keys.findIndex((k) => k.metric === "net.rx_total");
    this.txTotal = gen.keys.findIndex((k) => k.metric === "net.tx_total");
    this.ringStartMs = startMs - (historyRows - 1) * startInterval;
    this.rows = generated.map((row, i) => {
      const ts = startMs - (historyRows - 1 - i) * startInterval;
      return {
        ts,
        interval: startInterval,
        timeline: 0,
        values: this.withBursts(ts, row.values),
        holds: holdsOf(row.cadences, startInterval),
      };
    });
    if (flags.sleepGap) {
      this.rows = this.rows.filter(
        (r) => r.ts < startMs - 40_000 || r.ts > startMs - 25_000
      );
    }
    // The ring holds nothing inside a closed history gap (the sleep
    // within the last hour), as the engine's ring would not.
    const closedGaps = mockGaps(flags, startMs).filter(
      (g) => g.end_ms !== null
    );
    this.rows = this.rows.filter(
      (r) =>
        !closedGaps.some((g) => r.ts >= g.start_ms && r.ts < (g.end_ms ?? 0))
    );

    this.caps = capabilities(flags, gen.seriesPerModule());
    this.rxIdx = gen.keys.flatMap((k, i) => (k.metric === "net.rx" ? [i] : []));
    this.txIdx = gen.keys.flatMap((k, i) => (k.metric === "net.tx" ? [i] : []));
  }
  readonly withBursts = (ts: number, values: (number | null)[]) => {
    for (const b of burstsAt(ts, this.startMs)) {
      for (const [i, bps] of [
        [this.en0Rx, b.rxBps],
        [this.rxTotal, b.rxBps],
        [this.en0Tx, b.txBps],
        [this.txTotal, b.txBps],
      ] as const) {
        const v = values[i];
        if (v != null) values[i] = v + bps;
      }
    }
    return values;
  };
  readonly pushRow = (
    ts: number,
    values: (number | null)[],
    cadences: readonly number[],
    intervalMs: number
  ) => {
    while (
      this.rows.length > 0 &&
      (this.rows[this.rows.length - 1] as HistoryRow).ts >= ts
    ) {
      this.rows.pop();
    }
    this.rows.push({
      ts,
      interval: intervalMs,
      timeline: this.timeline,
      values,
      holds: holdsOf(cadences, intervalMs),
    });
    if (this.rows.length > 3600) this.rows = this.rows.slice(-3600);
  };
  readonly setReadFailing: MockTransport["setReadFailing"] = (key, failing) => {
    const i = this.gen.indexOf(key);
    if (i < 0) return;
    if (failing) this.gen.failing.add(i);
    else this.gen.failing.delete(i);
  };

  get methods() {
    return { setReadFailing: this.setReadFailing };
  }
}
