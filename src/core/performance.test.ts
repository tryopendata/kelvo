import { performanceChanges, performanceNextLever } from "./performance";
import { DEFAULT_MENU_BAR } from "./settings-patch";

const settings = (
  interval_ms: number,
  over: { slow_on_battery?: boolean } = {}
) => ({
  sampling: { interval_ms, slow_on_battery: over.slow_on_battery ?? false },
  modules: { power: { enabled: true } },
});

describe("performanceChanges (D-088)", () => {
  it("lists every lever at the default 1 s interval", () => {
    expect(performanceChanges(settings(1000))).toEqual([
      "Open windows update every 2 s",
      "Menu bar updates every 4 s with a window open",
      "Processes sampled every 30 s when no view lists them",
      "Interval doubles on battery",
      "Animations off",
    ]);
  });

  it("gives the backed-off figures under Low Power Mode", () => {
    expect(performanceChanges(settings(1000), "low_power_mode")).toEqual([
      "Menu bar updates every 8 s with a window open",
      "Processes sampled every 30 s when no view lists them",
      "Animations off",
    ]);
    expect(performanceChanges(settings(500), "low_power_mode")).toContain(
      "Open windows update every 2 s"
    );
  });

  it("leaves out what the interval already does", () => {
    expect(
      performanceChanges(settings(5000, { slow_on_battery: true }))
    ).toEqual([
      "Processes sampled every 30 s when no view lists them",
      "Animations off",
    ]);
    expect(
      performanceChanges(settings(30_000, { slow_on_battery: true }))
    ).toEqual(["Animations off"]);
  });

  it("leaves out the background's cadences, which apply without it (D-094)", () => {
    for (const interval of [500, 1000, 2000]) {
      const lines = performanceChanges(settings(interval));
      expect(lines.filter((l) => /background|Temperatures/.test(l))).toEqual(
        []
      );
    }
  });
});

describe("performanceNextLever", () => {
  it("names own menu bar items first, then the interval", () => {
    const own = {
      ...settings(1000),
      modules: { cpu: { enabled: true } },
      menu_bar: {
        ...DEFAULT_MENU_BAR,
        items: { ...DEFAULT_MENU_BAR.items, cpu: "graph" as const },
      },
    };
    expect(performanceNextLever(own)).toMatch(/Separate menu bar items/);
    expect(performanceNextLever(settings(1000))).toBe(
      "A longer sample interval saves more while a window is open."
    );
    expect(performanceNextLever(settings(2000))).toBeNull();
  });
});
