import type {
  MemoryUnit,
  NetworkUnit,
  Settings,
  TemperatureUnit,
} from "@core/generated/bindings";
import { SegmentedControl } from "~/components/segmented-control";
import { SettingsPanel, SettingsRow } from "~/components/settings-row";
import { useWriteSettings } from "~/hooks/use-write-settings";
import { useSettings } from "~/stores/settings-store";
import { AlertsPanel } from "./_components/alerts-panel";
import { GeneralPanel } from "./_components/general-panel";
import { MenuBarPanel } from "./_components/menu-bar-panel";
import { ModulesPanel } from "./_components/modules-panel";
import { SamplingPanel } from "./_components/sampling-panel";

const TEMPERATURE = [
  { value: "celsius", label: "°C" },
  { value: "fahrenheit", label: "°F" },
] as const satisfies readonly { value: TemperatureUnit; label: string }[];
const NETWORK = [
  { value: "bytes_per_sec", label: "MB/s" },
  { value: "bits_per_sec", label: "Mb/s" },
] as const satisfies readonly { value: NetworkUnit; label: string }[];
const MEMORY = [
  { value: "decimal", label: "GB" },
  { value: "binary", label: "GiB" },
] as const satisfies readonly { value: MemoryUnit; label: string }[];

function UnitsPanel({ units }: { units: Settings["units"] }) {
  const write = useWriteSettings();
  return (
    <SettingsPanel title="Units">
      <SettingsRow label="Temperature">
        <SegmentedControl
          ariaLabel="Temperature unit"
          options={TEMPERATURE}
          value={units.temperature}
          onChange={(temperature) => write({ units: { temperature } })}
        />
      </SettingsRow>
      <SettingsRow label="Network rate">
        <SegmentedControl
          ariaLabel="Network unit"
          options={NETWORK}
          value={units.network}
          onChange={(network) => write({ units: { network } })}
        />
      </SettingsRow>
      <SettingsRow label="Memory">
        <SegmentedControl
          ariaLabel="Memory base"
          options={MEMORY}
          value={units.memory}
          onChange={(memory) => write({ units: { memory } })}
        />
      </SettingsRow>
    </SettingsPanel>
  );
}

/**
 * Settings (plan 4.15). Every control reads the Rust-owned
 * settings mirror and writes one `update_settings` patch (D-050); nothing is
 * kept locally, so another window's change shows here at once.
 */
export default function SettingsRoute() {
  const settings = useSettings((s) => s);
  return (
    <div className="flex flex-col gap-5">
      <h1 className="font-[590] text-[22px] tracking-[-0.022em]">Settings</h1>
      {settings && (
        <div className="grid grid-cols-[repeat(auto-fit,minmax(380px,1fr))] items-start gap-5">
          <div className="flex flex-col gap-5">
            <ModulesPanel modules={settings.modules} />
            <MenuBarPanel
              modules={settings.modules}
              menuBar={settings.menu_bar}
            />
            <AlertsPanel alerts={settings.alerts} />
          </div>
          <div className="flex flex-col gap-5">
            <SamplingPanel
              sampling={settings.sampling}
              history={settings.history}
            />
            <UnitsPanel units={settings.units} />
            <GeneralPanel general={settings.general} />
          </div>
        </div>
      )}
    </div>
  );
}
