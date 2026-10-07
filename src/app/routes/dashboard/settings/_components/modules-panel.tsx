import type { MenuBarMode, Settings } from "@core/generated/bindings";
import {
  MENU_BAR_LABELS,
  menuBarModes,
  modulePatch,
  modulePresence,
  SETTINGS_MODULES,
  type SettingsModule,
} from "@core/settings-patch";
import { SettingsPanel } from "~/components/settings-row";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { Switch } from "~/components/ui/switch";
import { useWriteSettings } from "~/hooks/use-write-settings";
import { cn } from "~/lib/utils";
import { useHost } from "~/stores/host-store";
import type { Accent } from "~/widgets/lib/accent";
import { FIELD_LABEL } from "~/widgets/lib/classes";

const MODULE_ROWS: Record<SettingsModule, { name: string; accent: Accent }> = {
  cpu: { name: "CPU", accent: "cpu" },
  gpu: { name: "GPU", accent: "gpu" },
  memory: { name: "Memory", accent: "mem" },
  power: { name: "Power & Sensors", accent: "power" },
  network: { name: "Network", accent: "net" },
  disk: { name: "Disk", accent: "disk" },
  battery: { name: "Battery", accent: "battery" },
};

const ABSENT_TEXT = {
  not_present: "Not present on this Mac",
  unavailable: "Not available on this Mac",
} as const;

/**
 * "Modules": swatch, name, Menu bar select (the modes the module
 * allows) and the On switch. A module the host lacks stays listed, disabled.
 * Switching a module off greys its select; the mode is kept for when it
 * comes back.
 */
export function ModulesPanel({ modules }: { modules: Settings["modules"] }) {
  const caps = useHost((s) => s.capabilities);
  const write = useWriteSettings();

  return (
    <SettingsPanel title="Modules">
      <div className="flex min-h-[34px] items-center gap-3 border-border-subtle border-b px-4">
        <span className={cn(FIELD_LABEL, "flex-1")}>Module</span>
        <span className={cn(FIELD_LABEL, "w-[150px]")}>Menu bar</span>
        <span className={cn(FIELD_LABEL, "w-[30px]")}>On</span>
      </div>
      {SETTINGS_MODULES.map((m) => {
        const row = MODULE_ROWS[m];
        const setting = modules[m];
        const presence = modulePresence(caps?.modules[m], caps !== null);
        const present = presence === "present";
        const on = present && (setting?.enabled ?? false);
        const selectId = `settings-menubar-${m}`;
        return (
          <div
            key={m}
            className="flex min-h-12 items-center gap-3 border-border-subtle border-b px-4 text-[13px] last:border-b-0"
          >
            <span
              aria-hidden
              className={cn(
                "size-2 shrink-0 rounded-mark",
                !present && "opacity-40"
              )}
              style={{ background: `var(--color-${row.accent})` }}
            />
            <div className="flex min-w-0 flex-1 flex-col py-2">
              <label
                htmlFor={selectId}
                className={cn(!on && "text-muted-foreground")}
              >
                {row.name}
              </label>
              {!present && (
                <span className="font-normal text-[12px] text-muted-foreground">
                  {ABSENT_TEXT[presence]}
                </span>
              )}
            </div>
            <Select
              value={setting?.menu_bar}
              disabled={!on}
              onValueChange={(mode) =>
                write(modulePatch(m, { menu_bar: mode as MenuBarMode }))
              }
            >
              <SelectTrigger
                id={selectId}
                size="sm"
                aria-label={`${row.name} menu bar style`}
                className="w-[150px] px-2 text-[12px]"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {menuBarModes(m).map((mode) => (
                  <SelectItem key={mode} value={mode}>
                    {MENU_BAR_LABELS[mode]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Switch
              checked={on}
              disabled={!present}
              aria-label={`${row.name} enabled`}
              onCheckedChange={(enabled) => write(modulePatch(m, { enabled }))}
            />
          </div>
        );
      })}
    </SettingsPanel>
  );
}
