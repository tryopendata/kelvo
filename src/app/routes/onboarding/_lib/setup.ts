import type { Capabilities, HostInfo } from "@core/generated/bindings";
import {
  modulePresence,
  SETTINGS_MODULES,
  type SettingsModule,
} from "@core/settings-patch";

/** Disk starts off; every other present module starts on. */
const DEFAULT_OFF: ReadonlySet<SettingsModule> = new Set(["disk"]);

/** "M4 Pro detected · all sensors mapped" or "Mac17,4 detected · sensors not mapped yet". */
export function chipStatus(info: HostInfo | undefined): string | null {
  if (!info) return null;
  if (info.chip_known && info.chip) {
    return `${info.chip.replace(/^Apple /, "")} detected · all sensors mapped`;
  }
  return `${info.model ?? info.chip ?? "This Mac"} detected · sensors not mapped yet`;
}

/** Each module's switch before the user touches it, from capabilities. */
export function defaultEnabled(
  caps: Capabilities | null
): Record<SettingsModule, boolean> {
  const out = {} as Record<SettingsModule, boolean>;
  for (const m of SETTINGS_MODULES) {
    const present =
      modulePresence(caps?.modules[m], caps !== null) === "present";
    out[m] = present && !DEFAULT_OFF.has(m);
  }
  return out;
}
