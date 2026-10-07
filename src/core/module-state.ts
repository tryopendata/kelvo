/**
 * How a UI module shows on this host (plan 4.17): listed with data, listed
 * dimmed because the user switched it off, left out because the host lacks
 * it, or shown as "not available". The UI's Power & Sensors module is the
 * `power` and `sensors` capability pair; an unknown chip hides it entirely
 * (CPU and GPU watts still show on their own pages).
 */
import type {
  Capabilities,
  ModuleCap,
  Settings,
} from "@core/generated/bindings";

/** The seven modules the sidebar, popover and Overview list, in order. */
export const UI_MODULES = [
  "cpu",
  "gpu",
  "memory",
  "power",
  "network",
  "disk",
  "battery",
] as const;

export type UiModule = (typeof UI_MODULES)[number];

export const MODULE_NAMES: Record<UiModule, string> = {
  cpu: "CPU",
  gpu: "GPU",
  memory: "Memory",
  power: "Power & Sensors",
  network: "Network",
  disk: "Disk",
  battery: "Battery",
};

export type ModuleState =
  /** Measured and switched on. */
  | "on"
  /** The user switched it off: listed dimmed, no value. */
  | "disabled"
  /** The host does not have it (Battery on a Mac mini): left out. */
  | "absent"
  /** Unknown chip: Power & Sensors is hidden and a notice explains why. */
  | "unknown_chip"
  /** `missing_entitlement`: "Not available in this edition". */
  | "edition"
  /** Any other unsupported or unknown capability: "Not available". */
  | "unavailable";

function capState(cap: ModuleCap | undefined): ModuleState | "available" {
  if (cap === undefined || cap === "not_present") return "absent";
  if (cap === "unknown") return "unavailable";
  if ("available" in cap) return "available";
  if (cap.unsupported === "missing_entitlement") return "edition";
  if (cap.unsupported === "unknown_chip") return "unknown_chip";
  return "unavailable";
}

/**
 * State of one UI module. Before capabilities arrive every module reads
 * "on", so the first paint is not a flash of empty navigation.
 */
export function moduleState(
  caps: Capabilities | null,
  modules: Settings["modules"] | null,
  module: UiModule
): ModuleState {
  if (caps) {
    if (module === "power") {
      const sensors = capState(caps.modules.sensors);
      if (sensors === "unknown_chip") return "unknown_chip";
    }
    const state = capState(caps.modules[module]);
    if (state !== "available") return state;
  }
  return modules?.[module]?.enabled === false ? "disabled" : "on";
}

/** Modules that get an entry (sidebar) or a card (popover, Overview). */
export function isListed(state: ModuleState): boolean {
  return state !== "absent" && state !== "unknown_chip";
}

/** The sensor notice applies: the chip is not in the sensor map. */
export function isUnknownChip(caps: Capabilities | null): boolean {
  const s = caps?.modules.sensors;
  return typeof s === "object" && "unsupported" in s
    ? s.unsupported === "unknown_chip"
    : false;
}

/** Text for a card whose module cannot run here. */
export function unavailableText(state: ModuleState): string | null {
  if (state === "edition") return "Not available in this edition";
  if (state === "unavailable") return "Not available on this Mac";
  return null;
}
