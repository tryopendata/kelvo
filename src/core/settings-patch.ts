/**
 * `update_settings` patches built by Settings (plan 4.15) and onboarding
 * (4.16), plus the option lists those screens offer. Every write is a
 * partial patch (D-050): absent fields keep their value in Rust, so one
 * control never overwrites another window's change.
 */
import {
  type CommandError,
  ITEM_MODES,
  type ItemMode,
  type MenuBarSettings,
  type ModuleCap,
  type Module as ModuleId,
  type ModulePatch,
  type Readout,
  SAMPLING_INTERVALS_MS,
  SETTINGS_MODULES as SETTINGS_MODULE_ORDER,
  type SettingsPatch,
} from "@core/generated/bindings";

export { RETENTION_DAYS, SIZE_LIMITS_MB } from "@core/generated/bindings";

/** Modules with a settings entry: every module except `sensors` (it rides with Power). */
export type SettingsModule = Exclude<ModuleId, "sensors" | "unknown">;

/** `kelvo_schema::settings::SETTINGS_MODULES`, in settings row order. */
export const SETTINGS_MODULES: readonly SettingsModule[] =
  SETTINGS_MODULE_ORDER;

/** `SamplingSettings::INTERVALS_MS`. */
export const INTERVALS_MS = SAMPLING_INTERVALS_MS;

/** `ItemMode::allowed_for`: what each module's own-item select offers (D-102). */
export function itemModes(module: SettingsModule): readonly ItemMode[] {
  return ITEM_MODES[module];
}

/**
 * The own-item option's label. "Value" says what it shows where the module
 * has more than one kind of number: the Disk item is throughput, while the
 * Disk readout is % used.
 */
export function itemModeLabel(module: SettingsModule, mode: ItemMode): string {
  switch (mode) {
    case "off":
      return "Off";
    case "graph":
      return "Graph";
    case "cores":
      return "Per-core graph";
    case "value":
      if (module === "power") return "Watts";
      if (module === "network") return "Total rate";
      if (module === "disk") return "Read + write rate";
      return "Value";
  }
}

export function readoutPatch(readout: Readout, on: boolean): SettingsPatch {
  return { menu_bar: { readouts: { [readout]: on } } };
}

export function barPatch(
  module: "cpu" | "gpu" | "memory",
  on: boolean
): SettingsPatch {
  return { menu_bar: { bars: { [module]: on } } };
}

export function itemPatch(
  module: SettingsModule,
  mode: ItemMode
): SettingsPatch {
  return { menu_bar: { items: { [module]: mode } } };
}

export function modulePatch(
  module: SettingsModule,
  change: ModulePatch
): SettingsPatch {
  return { modules: { [module]: change } };
}

/**
 * Whether a module can be switched on here. Until capabilities arrive
 * (`known` false) the row stays usable.
 */
export type ModulePresence = "present" | "not_present" | "unavailable";

export function modulePresence(
  cap: ModuleCap | undefined,
  known: boolean
): ModulePresence {
  if (!known) return "present";
  if (cap === undefined || cap === "not_present") return "not_present";
  if (cap === "unknown" || "unsupported" in cap) return "unavailable";
  return "present";
}

/** Menu bar styles onboarding offers: combined and values, and graph per module. */
export type TrayStyle = "combined" | "values" | "graphs";

const NO_ITEMS: MenuBarSettings["items"] = {
  cpu: "off",
  gpu: "off",
  memory: "off",
  power: "off",
  network: "off",
  disk: "off",
  battery: "off",
};
const NO_READOUTS: MenuBarSettings["readouts"] = {
  cpu: false,
  gpu: false,
  memory: false,
  temperature: false,
  power: false,
  network: false,
  disk: false,
  battery: false,
};

/**
 * The menu bar a style sets (D-102). Combined: three bars and the
 * temperature. Values: CPU, GPU, Memory and the temperature as numbers.
 * Graph per module: CPU sparkline, memory gauge and network rates, each a
 * status item of its own (D-080), and nothing in the combined item.
 */
export function trayStyleMenuBar(style: TrayStyle): MenuBarSettings {
  const bars = style === "combined";
  if (style === "graphs") {
    return {
      bars: { cpu: false, gpu: false, memory: false },
      readouts: NO_READOUTS,
      items: { ...NO_ITEMS, cpu: "graph", memory: "graph", network: "graph" },
    };
  }
  const values = style === "values";
  return {
    bars: { cpu: bars, gpu: bars, memory: bars },
    readouts: {
      ...NO_READOUTS,
      cpu: values,
      gpu: values,
      memory: values,
      temperature: true,
    },
    items: NO_ITEMS,
  };
}

/** `MenuBarSettings::default()`: three bars and the temperature. */
export const DEFAULT_MENU_BAR: MenuBarSettings = trayStyleMenuBar("combined");

/**
 * The settings' menu bar. Rust always sends it; the generated type has it
 * optional only because a file from before D-102 decodes without it.
 */
export function menuBarOf(settings: {
  menu_bar?: MenuBarSettings;
}): MenuBarSettings {
  return settings.menu_bar ?? DEFAULT_MENU_BAR;
}

export interface OnboardingChoices {
  /** Modules switched on; a module missing from the map keeps its value. */
  enabled: Partial<Record<SettingsModule, boolean>>;
  style: TrayStyle;
  launchAtLogin: boolean;
}

/** Onboarding step 1 Continue: modules, menu bar style and launch at login. */
export function onboardingChoicesPatch(
  choices: OnboardingChoices
): SettingsPatch {
  const modules: Partial<Record<SettingsModule, ModulePatch>> = {};
  for (const m of SETTINGS_MODULES) {
    const enabled = choices.enabled[m];
    if (enabled !== undefined) modules[m] = { enabled };
  }
  return {
    modules,
    menu_bar: trayStyleMenuBar(choices.style),
    general: { launch_at_login: choices.launchAtLogin },
  };
}

/** Onboarding step 2 Done: the update switch, and onboarding is over. */
export function onboardingDonePatch(checkUpdates: boolean): SettingsPatch {
  return {
    general: { check_updates: checkUpdates },
    onboarding: { completed: true },
  };
}

/** Skip keeps every default and only marks onboarding done. */
export function onboardingSkipPatch(): SettingsPatch {
  return { onboarding: { completed: true } };
}

/** What a failed `update_settings` tells the user. Nothing changed in either case. */
export function settingsErrorText(error: CommandError): string {
  if (error.kind === "settings_not_saved") {
    return "Couldn't save settings. Nothing changed.";
  }
  if (error.kind === "invalid_settings") {
    return `Kelvo rejected that setting: ${error.message}`;
  }
  return "Couldn't change settings. Nothing changed.";
}
