import { eventLabel, mergeEvent, RecentEvents } from "./events";
import type { Event } from "./generated/bindings";

const ev = (ts: number, detail: Event["detail"], processes: string[] = []) => ({
  ts_ms: ts,
  start_ms: ts - 60_000,
  processes,
  detail,
});

describe("eventLabel", () => {
  it("names what happened and who was behind it", () => {
    expect(
      eventLabel(
        ev(0, { kind: "fans_ramped", from_rpm: 1450, to_rpm: 4210 }, [
          "kernel_task",
          "Xcode build",
        ])
      )
    ).toBe("Fans ramped up · kernel_task + Xcode build");
    expect(
      eventLabel(ev(0, { kind: "fans_ramped", from_rpm: 1, to_rpm: 2 }))
    ).toBe("Fans ramped up");
    expect(
      eventLabel(
        ev(
          0,
          {
            kind: "power_spike",
            component: "ane",
            watts: 1.4,
            baseline_watts: 0.1,
          },
          ["Photos face analysis", "photolibraryd"]
        )
      )
    ).toBe("ANE 1.4 W · Photos face analysis");
    expect(
      eventLabel(
        ev(0, { kind: "thermal_state", from: "nominal", to: "serious" })
      )
    ).toBe("Thermal state Serious");
    expect(
      eventLabel(
        ev(0, {
          kind: "sustained_process",
          process: "x264",
          cpu_pct: 141.6,
          secs: 300,
        })
      )
    ).toBe("x264 at 142% CPU for 5 min");
    expect(
      eventLabel(
        ev(0, {
          kind: "alert",
          rule_id: "r",
          rule_name: "n",
          cause: { type: "process_cpu", process: "ffmpeg", cpu_pct: 251 },
        })
      )
    ).toBe("Alert · ffmpeg at 251% CPU");
    expect(
      eventLabel(
        ev(0, {
          kind: "alert",
          rule_id: "r",
          rule_name: "n",
          cause: { type: "thermal_state", state: "critical" },
        })
      )
    ).toBe("Alert · thermal state Critical");
  });

  it("shows an unknown figure as ? rather than 0", () => {
    expect(
      eventLabel(
        ev(0, {
          kind: "power_spike",
          component: "package",
          watts: null,
          baseline_watts: null,
        })
      )
    ).toBe("Package ?");
  });
});

describe("mergeEvent", () => {
  const a = ev(1_000, { kind: "fans_ramped", from_rpm: 1, to_rpm: 2 });
  const b = ev(2_000, { kind: "thermal_state", from: null, to: "fair" });

  it("adds a new event in time order", () => {
    expect(mergeEvent([b], a)).toEqual([a, b]);
  });

  it("returns the same list for an event it already holds", () => {
    const list = [a, b];
    expect(mergeEvent(list, { ...b })).toBe(list);
  });

  it("keeps two kinds at one time apart", () => {
    const c = ev(1_000, { kind: "thermal_state", from: null, to: "fair" });
    expect(mergeEvent([a], c)).toHaveLength(2);
  });
});

describe("RecentEvents", () => {
  const a = ev(1_000, { kind: "fans_ramped", from_rpm: 1, to_rpm: 2 });
  const b = ev(2_000, { kind: "thermal_state", from: null, to: "fair" });
  const c = ev(3_000, { kind: "thermal_state", from: "fair", to: "nominal" });

  function recent(ttlMs = 10_000, max = 64) {
    let t = 0;
    const r = new RecentEvents(ttlMs, max, () => t);
    return {
      r,
      at: (ms: number) => {
        t = ms;
      },
    };
  }

  it("merges recent pushes into an answer that missed them, once", () => {
    const { r } = recent();
    r.add("h", b);
    expect(r.mergeInto("h", [a], 0, 10_000)).toEqual([a, b]);
    expect(r.mergeInto("h", [a, b], 0, 10_000)).toEqual([a, b]);
  });

  it("drops a push once its time to live has passed", () => {
    const { r, at } = recent(10_000);
    r.add("h", b);
    at(10_000);
    expect(r.mergeInto("h", [a], 0, 10_000)).toEqual([a, b]);
    at(10_001);
    expect(r.mergeInto("h", [a], 0, 10_000)).toEqual([a]);
  });

  it("keeps to the read's range and its host", () => {
    const { r } = recent();
    r.add("h", b);
    expect(r.mergeInto("h", [], 2_001, 10_000)).toEqual([]);
    expect(r.mergeInto("h", [], 0, 2_000)).toEqual([]);
    expect(r.mergeInto("other", [], 0, 10_000)).toEqual([]);
  });

  it("holds at most `max` per host, dropping the oldest", () => {
    const { r } = recent(10_000, 2);
    r.add("h", a);
    r.add("h", b);
    r.add("h", c);
    expect(r.mergeInto("h", [], 0, 10_000)).toEqual([b, c]);
  });
});
