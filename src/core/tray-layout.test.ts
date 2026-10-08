import type { MenuBarSettings } from "@core/generated/bindings";
import { DEFAULT_MENU_BAR, trayStyleMenuBar } from "./settings-patch";
import {
  combinedWidthPt,
  rateLine,
  type TrayReadings,
  type TrayUnits,
  tempText,
  trayLayout,
  WIDE_ITEM_PT,
  wattsText,
} from "./tray-layout";

const V: TrayReadings = {
  cpu: 18,
  gpu: 36,
  mem: 42,
  temp: 61,
  power: 14.8,
  netUp: 1.2e6,
  netDown: 38.4e6,
  diskRate: 4.1e6,
  diskUsed: 62,
  battery: 87,
};
const UNITS: TrayUnits = { temperature: "celsius", network: "bytes_per_sec" };
const ALL = () => true;

const withReadouts = (
  on: Partial<MenuBarSettings["readouts"]>,
  base = DEFAULT_MENU_BAR
): MenuBarSettings => ({ ...base, readouts: { ...base.readouts, ...on } });

describe("trayLayout", () => {
  it("draws the default as three bars and the bare temperature", () => {
    const { combined, items } = trayLayout(DEFAULT_MENU_BAR, ALL, V, UNITS);
    expect(items).toEqual([]);
    expect(combined?.bars).toEqual([18, 36, 42]);
    expect(combined?.readouts).toEqual([
      {
        readout: "temperature",
        kind: "marked",
        marker: { kind: "none" },
        text: "61°",
      },
    ]);
  });

  it("prints readouts in catalog order with their markers", () => {
    const menuBar = withReadouts({ disk: true, power: true, cpu: true });
    const readouts = trayLayout(menuBar, ALL, V, UNITS).combined?.readouts;
    expect(readouts?.map((r) => r.readout)).toEqual([
      "cpu",
      "temperature",
      "power",
      "disk",
    ]);
    expect(readouts?.[2]).toMatchObject({
      marker: { kind: "glyph", glyph: "bolt" },
      text: "14.8W",
    });
    expect(readouts?.[3]).toMatchObject({
      marker: { kind: "glyph", glyph: "drive" },
      text: "62%",
    });
  });

  it("drops what a disabled module would show; temperature rides with Power", () => {
    const menuBar = withReadouts({ power: true });
    const { combined } = trayLayout(
      menuBar,
      (m) => m !== "power" && m !== "gpu",
      V,
      UNITS
    );
    expect(combined?.bars).toEqual([18, 42]);
    expect(combined?.readouts).toEqual([]);
  });

  it("draws a gap as a dash, never a zero", () => {
    const gaps = { ...V, temp: null, power: null };
    const readouts = trayLayout(withReadouts({ power: true }), ALL, gaps, UNITS)
      .combined?.readouts;
    expect(readouts?.map((r) => (r.kind === "marked" ? r.text : ""))).toEqual([
      "–",
      "–",
    ]);
  });

  it("takes the combined item away when it is empty and an own item exists", () => {
    const graphs = trayLayout(trayStyleMenuBar("graphs"), ALL, V, UNITS);
    expect(graphs.combined).toBeNull();
    expect(graphs.items.map((i) => [i.module, i.kind])).toEqual([
      ["cpu", "spark"],
      ["memory", "gauge"],
      ["network", "rates"],
    ]);
  });

  it("keeps three empty tracks when nothing at all is on", () => {
    const nothing: MenuBarSettings = {
      ...trayStyleMenuBar("graphs"),
      items: { ...DEFAULT_MENU_BAR.items },
    };
    expect(trayLayout(nothing, ALL, V, UNITS)).toEqual({
      combined: { bars: [null, null, null], readouts: [] },
      items: [],
    });
  });

  it("labels own values by what they show", () => {
    const menuBar: MenuBarSettings = {
      ...DEFAULT_MENU_BAR,
      items: { ...DEFAULT_MENU_BAR.items, power: "value", disk: "value" },
    };
    expect(trayLayout(menuBar, ALL, V, UNITS).items).toEqual([
      { module: "power", kind: "value", label: "PWR", text: "14.8W" },
      { module: "disk", kind: "value", label: "DSK", text: "4.1MB" },
    ]);
  });
});

describe("tray formatters match the Rust renderer", () => {
  it("formats temperature, watts and rates", () => {
    expect(tempText(61.4, "celsius")).toBe("61°");
    expect(tempText(61.4, "fahrenheit")).toBe("143°");
    expect(wattsText(14.84)).toBe("14.8W");
    expect(wattsText(104.4)).toBe("104W");
    expect(rateLine(38.4e6, "bytes_per_sec")).toBe("38.4 MB/s");
    expect(rateLine(512e3, "bytes_per_sec")).toBe("512 KB/s");
    expect(rateLine(1.2e6, "bits_per_sec")).toBe("9.6 Mb/s");
  });
});

describe("combinedWidthPt", () => {
  it("keeps the default and a couple of readouts under the notch warning", () => {
    const width = (m: MenuBarSettings) =>
      combinedWidthPt(trayLayout(m, ALL, V, UNITS).combined);
    expect(width(DEFAULT_MENU_BAR)).toBeLessThan(WIDE_ITEM_PT);
    expect(
      width(withReadouts({ power: true, disk: true }))
    ).toBeLessThanOrEqual(WIDE_ITEM_PT);
    expect(width(withReadouts({ power: true, disk: true, cpu: true }))).toBe(
      // CPU's label and three characters, a 10 pt group gap, and the gap
      // after the bars widening from 4 pt (bare "61°") to 6 pt (a label).
      width(withReadouts({ power: true, disk: true })) + 10 + 3 * 7.2 + 10 + 2
    );
  });

  it("warns once everything is on", () => {
    const all = withReadouts({
      cpu: true,
      gpu: true,
      memory: true,
      power: true,
      network: true,
      disk: true,
      battery: true,
    });
    expect(
      combinedWidthPt(trayLayout(all, ALL, V, UNITS).combined)
    ).toBeGreaterThan(WIDE_ITEM_PT);
  });

  it("is zero without a combined item", () => {
    expect(combinedWidthPt(null)).toBe(0);
  });
});
