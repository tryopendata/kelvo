import type { LiveMsg, SeriesKey } from "@core/generated/bindings";
import { initialHostLive, reduceLive } from "@core/live-state";
import { ifaceRates, labelValue, linkRate, volumeCapacity } from "./selectors";

const SERIES: SeriesKey[] = [
  { metric: "net.rx", labels: [["iface", "en0"]] },
  { metric: "net.tx", labels: [["iface", "en0"]] },
  { metric: "net.link_rate", labels: [["iface", "en0"]] },
  { metric: "disk.total", labels: [["vol", "/System/Volumes/Data"]] },
  { metric: "disk.free", labels: [["vol", "/System/Volumes/Data"]] },
  { metric: "disk.total", labels: [["vol", "/"]] },
  { metric: "disk.free", labels: [["vol", "/"]] },
  { metric: "disk.used", labels: [["vol", "/"]] },
];

const VALUES = [38.4e6, 1.2e6, 1.2e9, 1e12, 600e9, 1e12, 612e9, 380e9];

const state = [
  { kind: "layout", layout_no: 1, series: SERIES },
  {
    kind: "frame",
    ts_ms: 1000,
    layout_no: 1,
    values: VALUES,
    held: VALUES,
  },
].reduce((s, m) => reduceLive(s, m as LiveMsg), initialHostLive("h1"));

describe("overview selectors", () => {
  it("reads labels out of a display key", () => {
    expect(labelValue("disk.free{vol=/}", "vol")).toBe("/");
    expect(labelValue("cpu.total", "vol")).toBeUndefined();
  });

  it("flattens interface rates and reads the link speed", () => {
    expect(ifaceRates(state)).toEqual({ "rx|en0": 38.4e6, "tx|en0": 1.2e6 });
    expect(linkRate(state, "en0")).toBe(1.2e9);
    expect(linkRate(state, null)).toBeNull();
  });

  it("reads one volume's capacity, used straight from disk.used", () => {
    expect(volumeCapacity(state, "/")).toEqual({
      total: 1e12,
      free: 612e9,
      used: 380e9,
    });
    expect(volumeCapacity(state, null)).toEqual({
      total: null,
      free: null,
      used: null,
    });
  });
});
