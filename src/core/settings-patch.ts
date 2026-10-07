/**
 * `update_settings` patches built by Settings (plan 4.15) and onboarding
 * (4.16), plus the option lists those screens offer. Every write is a
 * partial patch (D-050): absent fields keep their value in Rust, so one
 * control never overwrites another window's change.
 */
import {
  type CommandError,
  MENU_BAR_MODES,
  type MenuBarMode,
  type ModuleCap,
  type Module as ModuleId,
  type ModulePatch,
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

export const MENU_BAR_LABELS: Record<MenuBarMode, string> = {
  in_combined: "In combined item",
  value_label: "Value + label",
  temp_in_combined: "Temp in combined",
  watts_value: "Watts as value",
  own_graph: "Own item: graph",
  own_value: "Own item: value",
  own_cores: "Own item: cores",
  hidden: "Hidden",
};

/** `MenuBarMode::allowed_for`: what each module's Menu bar select offers (4.2). */
export function menuBarModes(module: SettingsModule): readonly MenuBarMode[] {
  return MENU_BAR_MODES[module];
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

/** Menu bar styles: combined and values, and graph per module. */
export type TrayStyle = "combined" | "values" | "graphs";

/**
 * Per-module menu bar modes for a style (4.2): Combined puts CPU, GPU and
 * Memory in the combined item, Values gives each its own value and label.
 * Both keep the SoC temperature in the item and hide the rest. Graph per
 * module is the "Graphs" row: CPU sparkline, memory gauge and network
 * rates, each a status item of its own (D-080), and the rest hidden.
 */
export function trayStyleModes(
  style: TrayStyle
): Record<SettingsModule, MenuBarMode> {
  if (style === "graphs") {
    return {
      cpu: "own_graph",
      gpu: "hidden",
      memory: "own_graph",
      power: "hidden",
      network: "own_graph",
      disk: "hidden",
      battery: "hidden",
    };
  }
  const bar: MenuBarMode = style === "combined" ? "in_combined" : "value_label";
  return {
    cpu: bar,
    gpu: bar,
    memory: bar,
    power: "temp_in_combined",
    network: "hidden",
    disk: "hidden",
    battery: "hidden",
  };
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
  const modes = trayStyleModes(choices.style);
  const modules: Partial<Record<SettingsModule, ModulePatch>> = {};
  for (const m of SETTINGS_MODULES) {
    const enabled = choices.enabled[m];
    modules[m] =
      enabled === undefined
        ? { menu_bar: modes[m] }
        : { enabled, menu_bar: modes[m] };
  }
  return { modules, general: { launch_at_login: choices.launchAtLogin } };
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
