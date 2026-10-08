import type {
  Appearance,
  Settings,
  UpdateStatus,
} from "@core/generated/bindings";
import { useState } from "react";
import { SettingsPanel, SettingsRow } from "~/components/settings-row";
import { Button } from "~/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { Switch } from "~/components/ui/switch";
import { useWriteSettings } from "~/hooks/use-write-settings";
import { useTransport } from "~/lib/transport-context";
import { version } from "../../../../../../package.json";

const APPEARANCES: { value: Appearance; label: string }[] = [
  { value: "system", label: "Match system" },
  { value: "dark", label: "Dark" },
  { value: "light", label: "Light" },
];

const UPDATE_TEXT: Record<UpdateStatus["kind"], string> = {
  not_configured: "This build has no update source yet",
  disabled: "Automatic checks are off, so nothing was requested",
};

/** "General", plus the update switch and version row (4.15). */
export function GeneralPanel({ general }: { general: Settings["general"] }) {
  const transport = useTransport();
  const write = useWriteSettings();
  const [status, setStatus] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  const checkNow = async () => {
    setChecking(true);
    const result = await transport.checkForUpdates().catch((err: unknown) => {
      console.error("[settings] check_for_updates failed", err);
      return null;
    });
    setChecking(false);
    setStatus(result ? UPDATE_TEXT[result.kind] : "Couldn't check for updates");
  };

  return (
    <SettingsPanel title="General">
      <SettingsRow label="Launch at login">
        <Switch
          checked={general.launch_at_login}
          aria-label="Launch at login"
          onCheckedChange={(on) => write({ general: { launch_at_login: on } })}
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
      <SettingsRow
        label="Check for updates automatically"
        sub="The only network request Kelvo makes"
      >
        <Switch
          checked={general.check_updates}
          aria-label="Check for updates automatically"
          onCheckedChange={(on) => write({ general: { check_updates: on } })}
        />
      </SettingsRow>
      <SettingsRow label="Version" sub={status ?? undefined}>
        <span className="figures text-[12px] text-fg-subtle">{version}</span>
        <Button
          variant="outline"
          size="sm"
          disabled={checking}
          onClick={checkNow}
        >
          Check now
        </Button>
      </SettingsRow>
    </SettingsPanel>
  );
}
