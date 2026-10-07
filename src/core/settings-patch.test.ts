import {
  menuBarModes,
  modulePatch,
  modulePresence,
  onboardingChoicesPatch,
  onboardingDonePatch,
  onboardingSkipPatch,
  settingsErrorText,
  trayStyleModes,
} from "./settings-patch";

describe("menuBarModes", () => {
  it("offers what MenuBarMode::allowed_for allows per module", () => {
    expect(menuBarModes("cpu")).toEqual([
      "in_combined",
      "value_label",
      "own_graph",
      "own_cores",
      "own_value",
      "hidden",
    ]);
    expect(menuBarModes("gpu")).toEqual([
      "in_combined",
      "value_label",
      "own_graph",
      "own_value",
      "hidden",
    ]);
    expect(menuBarModes("power")).toEqual([
      "temp_in_combined",
      "watts_value",
      "own_value",
      "hidden",
    ]);
    expect(menuBarModes("network")).toEqual([
      "value_label",
      "own_graph",
      "own_value",
      "hidden",
    ]);
    expect(menuBarModes("battery")).toEqual([
      "value_label",
      "own_value",
      "hidden",
    ]);
  });

  it("presets the Graphs row for Graph per module", () => {
    expect(trayStyleModes("graphs")).toEqual({
      cpu: "own_graph",
      gpu: "hidden",
      memory: "own_graph",
      power: "hidden",
      network: "own_graph",
      disk: "hidden",
      battery: "hidden",
    });
  });

  it("never offers a mode outside the module's list for any tray style", () => {
    for (const style of ["combined", "values", "graphs"] as const) {
      for (const [m, mode] of Object.entries(trayStyleModes(style))) {
        expect(menuBarModes(m as "cpu")).toContain(mode);
      }
    }
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
  it("Continue sends each module's switch with the style's menu bar mode", () => {
    const patch = onboardingChoicesPatch({
      enabled: { cpu: true, disk: false, battery: true },
      style: "values",
      launchAtLogin: false,
    });
    expect(patch.general).toEqual({ launch_at_login: false });
    expect(patch.modules?.cpu).toEqual({
      enabled: true,
      menu_bar: "value_label",
    });
    expect(patch.modules?.disk).toEqual({ enabled: false, menu_bar: "hidden" });
    expect(patch.modules?.power).toEqual({ menu_bar: "temp_in_combined" });
  });

  it("Combined puts CPU, GPU and Memory in the combined item", () => {
    const modes = trayStyleModes("combined");
    expect([modes.cpu, modes.gpu, modes.memory]).toEqual([
      "in_combined",
      "in_combined",
      "in_combined",
    ]);
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
