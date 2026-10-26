import type { Event } from "@core/generated/bindings";
import { eventRange, groupEvents } from "./events";

const event = (
  ts: number,
  detail: Event["detail"] = {
    kind: "fans_ramped",
    from_rpm: 1000,
    to_rpm: 2000,
  }
): Event => ({ ts_ms: ts, start_ms: ts - 60_000, detail, processes: [] });

describe("Timeline annotations", () => {
  it("preserves every dense marker while prioritizing a serious thermal transition", () => {
    const events = [
      event(100_000),
      event(105_000, { kind: "thermal_state", from: "fair", to: "serious" }),
      event(110_000),
    ];
    const groups = groupEvents(events, 0, 3_600_000, 1000);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.members).toEqual(events);
    expect(groups[0]?.representative).toBe(events[1]);
    expect(groupEvents(events, 90_000, 120_000, 1000)).toHaveLength(3);
  });
  it("uses half-open bounds and never clamps stale markers onto edges", () => {
    const groups = groupEvents(
      [event(-1), event(0), event(100), event(101)],
      0,
      100,
      1000
    );
    expect(
      groups.flatMap((group) => group.members).map((member) => member.ts_ms)
    ).toEqual([0]);
  });
  it("does not promote recovery to a warning", () => {
    const recovery = event(105_000, {
      kind: "thermal_state",
      from: "critical",
      to: "nominal",
    });
    expect(
      groupEvents([event(100_000), recovery], 0, 3_600_000, 1000)[0]
        ?.representative.detail.kind
    ).toBe("fans_ramped");
  });
  it("uses episode start through detection with padding, not an invented end", () => {
    expect(eventRange([event(600_000)], 10_000, 3_600_000)).toEqual({
      fromMs: 480_000,
      toMs: 660_000,
    });
    const instant = { ...event(600_000), start_ms: 600_000 };
    expect(eventRange([instant], 10_000, 3_600_000)).toEqual({
      fromMs: 300_000,
      toMs: 900_000,
    });
  });
});
