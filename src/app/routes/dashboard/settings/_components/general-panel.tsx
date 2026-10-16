import type { Appearance, Settings } from "@core/generated/bindings";
import { SettingsPanel, SettingsRow } from "~/components/settings-row";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { Switch } from "~/components/ui/switch";
import { useWriteSettings } from "~/hooks/use-write-settings";
import { UpdateRow } from "./update-row";

const APPEARANCES: { value: Appearance; label: string }[] = [
  { value: "system", label: "Match system" },
  { value: "dark", label: "Dark" },
  { value: "light", label: "Light" },
];

/** "General", plus the update switch and version row (4.15). */
export function GeneralPanel({ general }: { general: Settings["general"] }) {
  const write = useWriteSettings();

  return (
    <SettingsPanel title="General">
      <SettingsRow label="Launch at login">
        <Switch
          checked={general.launch_at_login}
          aria-label="Launch at login"
          onCheckedChange={(on) => write({ general: { launch_at_login: on } })}
        />
      </SettingsRow>
      <SettingsRow
        label="Show menu-bar icon"
        sub="Open Kelvo from Finder to show its dashboard when hidden."
      >
        <Switch
          checked={general.show_menu_bar ?? true}
          aria-label="Show menu-bar icon"
          onCheckedChange={(on) => write({ general: { show_menu_bar: on } })}
        />
      </SettingsRow>
      <SettingsRow label="Show in Dock">
        <Switch
          checked={general.show_in_dock}
          aria-label="Show in Dock"
          onCheckedChange={(on) => write({ general: { show_in_dock: on } })}
        />
      </SettingsRow>
      <SettingsRow label="Appearance" htmlFor="settings-appearance">
        <Select
          value={general.appearance}
          onValueChange={(v) =>
            write({ general: { appearance: v as Appearance } })
          }
        >
          <SelectTrigger
            id="settings-appearance"
            size="sm"
            className="px-2 text-[12px]"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {APPEARANCES.map((a) => (
              <SelectItem key={a.value} value={a.value}>
                {a.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </SettingsRow>
      <SettingsRow label="Check for updates automatically">
        <Switch
          checked={general.check_updates}
          aria-label="Check for updates automatically"
          onCheckedChange={(on) => write({ general: { check_updates: on } })}
        />
      </SettingsRow>
      <UpdateRow />
    </SettingsPanel>
  );
}
