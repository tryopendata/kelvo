import type {
  MenuBarSettings,
  Readout,
  Settings,
} from "@core/generated/bindings";
import { READOUTS } from "@core/generated/bindings";
import {
  barPatch,
  itemModeLabel,
  itemModes,
  itemPatch,
  type ModulePresence,
  menuBarOf,
  modulePresence,
  readoutPatch,
  SETTINGS_MODULES,
  type SettingsModule,
} from "@core/settings-patch";
import {
  combinedWidthPt,
  pctText,
  rateLine,
  readoutModule,
  type TrayMarker,
  type TrayReadings,
  type TrayUnits,
  tempText,
  trayLayout,
  WIDE_ITEM_PT,
  wattsText,
} from "@core/tray-layout";
import { useId } from "react";
import { TrayMarkerIcon, TrayPreview } from "~/components/tray-preview";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { Switch } from "~/components/ui/switch";
import { ToggleGroup, ToggleGroupItem } from "~/components/ui/toggle-group";
import { useTrayReadings, useTrayUnits } from "~/hooks/use-tray-readings";
import { useWriteSettings } from "~/hooks/use-write-settings";
import { cn } from "~/lib/utils";
import { useHost } from "~/stores/host-store";
import { FIELD_LABEL } from "~/widgets/lib/classes";
import { ABSENT_TEXT, MODULE_ROWS } from "./modules-panel";

const BAR_MODULES = ["cpu", "gpu", "memory"] as const;

const READOUT_ROWS: Record<
  Readout,
  { name: string; spoken: string; marker: TrayMarker }
> = {
  cpu: { name: "CPU", spoken: "CPU", marker: { kind: "label", text: "CPU" } },
  gpu: { name: "GPU", spoken: "GPU", marker: { kind: "label", text: "GPU" } },
  memory: {
    name: "Memory",
    spoken: "memory",
    marker: { kind: "label", text: "MEM" },
  },
  temperature: {
    name: "Hottest temperature",
    spoken: "the hottest temperature",
    marker: { kind: "none" },
  },
  power: {
    name: "Power",
    spoken: "power",
    marker: { kind: "glyph", glyph: "bolt" },
  },
  network: {
    name: "Network",
    spoken: "network rates",
    marker: { kind: "none" },
  },
  disk: {
    name: "Disk used",
    spoken: "disk used",
    marker: { kind: "glyph", glyph: "drive" },
  },
  battery: {
    name: "Battery",
    spoken: "battery",
    marker: { kind: "label", text: "BAT" },
  },
};

/** What the readout prints now, as the menu bar formats it. */
function readoutValue(r: Readout, v: TrayReadings, units: TrayUnits): string {
  switch (r) {
    case "cpu":
      return pctText(v.cpu);
    case "gpu":
      return pctText(v.gpu);
    case "memory":
      return pctText(v.mem);
    case "temperature":
      return tempText(v.temp, units.temperature);
    case "power":
      return wattsText(v.power);
    case "network":
      return `↑ ${rateLine(v.netUp, units.network)}  ↓ ${rateLine(v.netDown, units.network)}`;
    case "disk":
      return pctText(v.diskUsed);
    case "battery":
      return pctText(v.battery);
  }
}

/** Why a module's controls are disabled, or null when they are not. */
function offText(
  m: SettingsModule,
  presence: ModulePresence,
  enabled: boolean
) {
  if (presence !== "present") return ABSENT_TEXT[presence];
  if (!enabled) return `Turn on ${MODULE_ROWS[m].name} in Modules to use this`;
  return null;
}

/** The marker column: the readout's marker as the menu bar draws it, muted. */
function MarkerCell({ readout }: { readout: Readout }) {
  const { marker } = READOUT_ROWS[readout];
  return (
    <span
      aria-hidden
      className="flex w-5 shrink-0 items-center justify-center text-muted-foreground"
    >
      {readout === "network" ? (
        <span className="font-tray text-[11px]">↑↓</span>
      ) : readout === "temperature" ? (
        <span className="font-tray text-[11px]">°</span>
      ) : (
        <TrayMarkerIcon marker={marker} />
      )}
    </span>
  );
}

const ROW =
  "flex min-h-10 items-center gap-3 border-border-subtle border-b px-4 text-[13px]";

/**
 * "Menu bar" (D-102): a live preview of the menu bar, then the three
 * independent choices it is built from. Bars, the values printed after
 * them, and modules' separate items. Every control writes a one-key patch.
 */
export function MenuBarPanel({
  modules,
  menuBar: stored,
}: {
  modules: Settings["modules"];
  menuBar: MenuBarSettings | undefined;
}) {
  const menuBar = menuBarOf({ menu_bar: stored });
  const caps = useHost((s) => s.capabilities);
  const write = useWriteSettings();
  const readings = useTrayReadings();
  const units = useTrayUnits();
  const titleId = useId();

  const presence = (m: SettingsModule) =>
    modulePresence(caps?.modules[m], caps !== null);
  const enabled = (m: SettingsModule) =>
    presence(m) === "present" && (modules[m]?.enabled ?? false);

  const layout = trayLayout(menuBar, enabled, readings, units);
  const wide = combinedWidthPt(layout.combined) > WIDE_ITEM_PT;

  const barsOn = BAR_MODULES.filter((m) => enabled(m) && menuBar.bars[m]);
  const barsOff = BAR_MODULES.filter((m) => !enabled(m));

  return (
    <section aria-labelledby={titleId} className="flex flex-col gap-2.5">
      <h2 id={titleId} className="font-[590] text-[14px]">
        Menu bar
      </h2>
      {/* No overflow-hidden here: it would stop the preview sticking. */}
      <div className="rounded-card border border-border bg-card">
        <div
          data-testid="menu-bar-preview"
          className="sticky top-0 z-10 flex flex-col gap-2 rounded-t-card border-border-subtle border-b bg-card px-4 py-3"
        >
          <TrayPreview layout={layout} />
          {wide && (
            <span className="font-normal text-[12px] text-muted-foreground">
              Wide items can end up hidden behind the camera on MacBooks with a
              notch.
            </span>
          )}
        </div>

        <div className="flex min-h-12 flex-wrap items-center gap-3 border-border-subtle border-b px-4 text-[13px]">
          <div className="flex min-w-0 flex-1 flex-col gap-0.5 py-2">
            <span>Bars</span>
            {barsOff.length > 0 && (
              <span className="font-normal text-[12px] text-muted-foreground">
                {barsOffText(barsOff)}
              </span>
            )}
          </div>
          <ToggleGroup
            type="multiple"
            size="sm"
            aria-label="Bars in the menu bar"
            value={[...barsOn]}
            onValueChange={(next) => {
              for (const m of BAR_MODULES) {
                const on = next.includes(m);
                if (on !== barsOn.includes(m)) write(barPatch(m, on));
              }
            }}
          >
            {BAR_MODULES.map((m) => (
              <ToggleGroupItem
                key={m}
                value={m}
                disabled={!enabled(m)}
                className="px-2.5"
              >
                {MODULE_ROWS[m].name}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </div>

        <div className={cn(FIELD_LABEL, "px-4 pt-3 pb-1")}>
          Values after the bars
        </div>
        {READOUTS.map((r) => {
          const m = readoutModule(r);
          const off = offText(m, presence(m), enabled(m));
          const row = READOUT_ROWS[r];
          const id = `settings-readout-${r}`;
          return (
            <div key={r} className={ROW}>
              <MarkerCell readout={r} />
              <div className="flex min-w-0 flex-1 flex-col py-2">
                <label
                  htmlFor={id}
                  className={cn(off && "text-muted-foreground")}
                >
                  {row.name}
                </label>
                {off && (
                  <span className="font-normal text-[12px] text-muted-foreground">
                    {off}
                  </span>
                )}
              </div>
              {!off && (
                <span className="figures whitespace-pre text-[12px] text-muted-foreground">
                  {readoutValue(r, readings, units)}
                </span>
              )}
              <Switch
                id={id}
                checked={!off && menuBar.readouts[r]}
                disabled={off !== null}
                aria-label={`Show ${row.spoken} in the menu bar`}
                onCheckedChange={(on) => write(readoutPatch(r, on))}
              />
            </div>
          );
        })}

        <div className={cn(FIELD_LABEL, "px-4 pt-3 pb-1")}>Separate items</div>
        {SETTINGS_MODULES.map((m) => {
          const off = offText(m, presence(m), enabled(m));
          const id = `settings-item-${m}`;
          return (
            <div key={m} className={ROW}>
              <div className="flex min-w-0 flex-1 flex-col py-2">
                <label
                  htmlFor={id}
                  className={cn(off && "text-muted-foreground")}
                >
                  {MODULE_ROWS[m].name}
                </label>
                {off && (
                  <span className="font-normal text-[12px] text-muted-foreground">
                    {off}
                  </span>
                )}
              </div>
              <Select
                value={off ? "off" : menuBar.items[m]}
                disabled={off !== null}
                onValueChange={(mode) =>
                  write(
                    itemPatch(m, mode as MenuBarSettings["items"][typeof m])
                  )
                }
              >
                <SelectTrigger
                  id={id}
                  size="sm"
                  aria-label={`${MODULE_ROWS[m].name} separate item`}
                  className="w-[150px] px-2 text-[12px]"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {itemModes(m).map((mode) => (
                    <SelectItem key={mode} value={mode}>
                      {itemModeLabel(m, mode)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          );
        })}
        <p className="px-4 py-3 font-normal text-[12px] text-muted-foreground">
          Each gets its own place in the menu bar, left of the bars. ⌘-drag to
          reorder.
        </p>
      </div>
    </section>
  );
}

function barsOffText(off: readonly (typeof BAR_MODULES)[number][]): string {
  const names = off.map((m) => MODULE_ROWS[m].name);
  const list =
    names.length === 1
      ? names[0]
      : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
  return `Turn on ${list} in Modules to show ${names.length === 1 ? "its bar" : "their bars"}`;
}
