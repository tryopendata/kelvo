import type { Capabilities, Settings } from "@core/generated/bindings";
import {
  isListed,
  isUnknownChip,
  moduleState,
  unavailableText,
} from "./module-state";

const on = { available: { series: 1 } } as const;

function caps(modules: Capabilities["modules"]): Capabilities {
  return {
    revision: 1,
    modules: {
      cpu: on,
      gpu: on,
      memory: on,
      power: on,
      sensors: on,
      network: on,
      disk: on,
      battery: on,
      ...modules,
    },
  };
}

const settings = (
  off: (keyof Settings["modules"])[] = []
): Settings["modules"] =>
  Object.fromEntries(
    ["cpu", "gpu", "memory", "power", "network", "disk", "battery"].map((m) => [
      m,
      { enabled: !off.includes(m as never), menu_bar: "hidden" as const },
    ])
  ) as unknown as Settings["modules"];

describe("moduleState", () => {
  it("reads every module as on before capabilities arrive", () => {
    expect(moduleState(null, null, "battery")).toBe("on");
  });

  it("leaves out a module the host does not have", () => {
    const c = caps({ battery: "not_present" });
    expect(moduleState(c, settings(), "battery")).toBe("absent");
    expect(isListed("absent")).toBe(false);
  });

  it("hides Power & Sensors on an unknown chip, even with power available", () => {
    const c = caps({ sensors: { unsupported: "unknown_chip" } });
    expect(moduleState(c, settings(), "power")).toBe("unknown_chip");
    expect(moduleState(c, settings(), "cpu")).toBe("on");
    expect(isListed("unknown_chip")).toBe(false);
    expect(isUnknownChip(c)).toBe(true);
    expect(isUnknownChip(caps({}))).toBe(false);
  });

  it("marks a module switched off in settings as disabled, still listed", () => {
    expect(moduleState(caps({}), settings(["gpu"]), "gpu")).toBe("disabled");
    expect(isListed("disabled")).toBe(true);
  });

  it("separates a missing entitlement from other unsupported reasons", () => {
    const c = caps({
      power: { unsupported: "missing_entitlement" },
      disk: "unknown",
    });
    expect(moduleState(c, settings(), "power")).toBe("edition");
    expect(moduleState(c, settings(), "disk")).toBe("unavailable");
    expect(unavailableText("edition")).toBe("Not available in this edition");
    expect(unavailableText("unavailable")).toBe("Not available on this Mac");
    expect(unavailableText("on")).toBeNull();
  });
});
