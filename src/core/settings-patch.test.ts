import { ITEM_MODES } from "@core/generated/bindings";
import {
  barPatch,
  itemModeLabel,
  itemModes,
  itemPatch,
  menuBarOf,
  modulePatch,
  modulePresence,
  onboardingChoicesPatch,
  onboardingDonePatch,
  onboardingSkipPatch,
  readoutPatch,
  SETTINGS_MODULES,
  settingsErrorText,
  trayStyleMenuBar,
} from "./settings-patch";

describe("item modes", () => {
  it("offers what ItemMode::allowed_for allows per module", () => {
    expect(itemModes("cpu")).toEqual(["off", "value", "graph", "cores"]);
    expect(itemModes("gpu")).toEqual(["off", "value", "graph"]);
    expect(itemModes("network")).toEqual(["off", "value", "graph"]);
    expect(itemModes("power")).toEqual(["off", "value"]);
    expect(itemModes("disk")).toEqual(["off", "value"]);
    expect(itemModes("battery")).toEqual(["off", "value"]);
  });

  it("says what the value is where a module has more than one number", () => {
    expect(itemModeLabel("disk", "value")).toBe("Read + write rate");
    expect(itemModeLabel("network", "value")).toBe("Total rate");
    expect(itemModeLabel("power", "value")).toBe("Watts");
    expect(itemModeLabel("cpu", "value")).toBe("Value");
    expect(itemModeLabel("cpu", "cores")).toBe("Per-core graph");
  });

  it("never presets a mode outside the module's list", () => {
    for (const style of ["combined", "values", "graphs"] as const) {
      const { items } = trayStyleMenuBar(style);
      for (const m of SETTINGS_MODULES) {
        expect(ITEM_MODES[m]).toContain(items[m]);
      }
    }
  });
});

describe("menu bar patches", () => {
  it("each control writes one key", () => {
    expect(readoutPatch("power", true)).toEqual({
      menu_bar: { readouts: { power: true } },
    });
    expect(barPatch("gpu", false)).toEqual({
      menu_bar: { bars: { gpu: false } },
    });
    expect(itemPatch("cpu", "cores")).toEqual({
      menu_bar: { items: { cpu: "cores" } },
    });
  });

  it("a settings value without a menu bar reads as the default", () => {
    expect(menuBarOf({})).toEqual(trayStyleMenuBar("combined"));
  });
});

describe("modulePatch", () => {
  it("touches one module and nothing else", () => {
    expect(modulePatch("disk", { enabled: true })).toEqual({
      modules: { disk: { enabled: true } },
    });
  });
});

describe("modulePresence", () => {
  it("keeps rows usable until capabilities are known", () => {
    expect(modulePresence(undefined, false)).toBe("present");
  });

  it("tells absent hardware from a collector that cannot run", () => {
    expect(modulePresence("not_present", true)).toBe("not_present");
    expect(modulePresence(undefined, true)).toBe("not_present");
    expect(modulePresence({ unsupported: "no_hardware" }, true)).toBe(
      "unavailable"
    );
    expect(modulePresence({ available: { series: 4 } }, true)).toBe("present");
  });
});

describe("onboarding patches", () => {
  it("Continue sends each module's switch and the style's menu bar", () => {
    const patch = onboardingChoicesPatch({
      enabled: { cpu: true, disk: false, battery: true },
      style: "values",
      launchAtLogin: false,
    });
    expect(patch.general).toEqual({ launch_at_login: false });
    expect(patch.modules).toEqual({
      cpu: { enabled: true },
      disk: { enabled: false },
      battery: { enabled: true },
    });
    expect(patch.menu_bar).toEqual(trayStyleMenuBar("values"));
  });

  it("the presets: Combined is bars and the temperature, Values the numbers", () => {
    const combined = trayStyleMenuBar("combined");
    expect(combined.bars).toEqual({ cpu: true, gpu: true, memory: true });
    expect(Object.entries(combined.readouts).filter(([, on]) => on)).toEqual([
      ["temperature", true],
    ]);
    const values = trayStyleMenuBar("values");
    expect(values.bars).toEqual({ cpu: false, gpu: false, memory: false });
    expect(values.readouts).toMatchObject({
      cpu: true,
      gpu: true,
      memory: true,
      temperature: true,
      power: false,
    });
  });

  it("Done and Skip mark onboarding completed", () => {
    expect(onboardingDonePatch(false)).toEqual({
      general: { check_updates: false },
      onboarding: { completed: true },
    });
    expect(onboardingSkipPatch()).toEqual({ onboarding: { completed: true } });
  });
});

describe("settingsErrorText", () => {
  it("says nothing changed when the file could not be written", () => {
    expect(
      settingsErrorText({ kind: "settings_not_saved", message: "disk full" })
    ).toBe("Couldn't save settings. Nothing changed.");
  });
});
