import type { HostInfo } from "@core/generated/bindings";
import {
  modulePresence,
  type OnboardingChoices,
  SETTINGS_MODULES,
  type SettingsModule,
  type TrayStyle,
} from "@core/settings-patch";
import { useState } from "react";
import {
  type ModuleToggleItem,
  ModuleToggleList,
} from "~/components/module-toggle-list";
import { TrayStyleGroup } from "~/components/tray-style-option";
import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import { cn } from "~/lib/utils";
import { useHost } from "~/stores/host-store";
import type { Accent } from "~/widgets/lib/accent";
import { FIELD_LABEL } from "~/widgets/lib/classes";
import { chipStatus, defaultEnabled } from "../_lib/setup";
import { OnboardingFrame } from "./onboarding-frame";
import { TrayStyleCards } from "./tray-style-cards";

const MODULES: Record<
  SettingsModule,
  { label: string; description: string; accent: Accent }
> = {
  cpu: { label: "CPU", description: "Load, clusters, per-core", accent: "cpu" },
  gpu: {
    label: "GPU",
    description: "Utilization and frequency",
    accent: "gpu",
  },
  memory: {
    label: "Memory",
    description: "Pressure, composition, swap",
    accent: "mem",
  },
  power: {
    label: "Power & Sensors",
    description: "Watts, SoC zones, fans",
    accent: "power",
  },
  network: {
    label: "Network",
    description: "Rates per interface",
    accent: "net",
  },
  disk: {
    label: "Disk",
    description: "Throughput and capacity",
    accent: "disk",
  },
  battery: {
    label: "Battery",
    description: "Charge, health, cycles",
    accent: "battery",
  },
};

/** Step 1 of 2: modules, menu bar style, launch at login. */
export function SetupStep({
  hostInfo,
  busy,
  onSkip,
  onContinue,
}: {
  hostInfo: HostInfo | undefined;
  busy: boolean;
  onSkip: () => void;
  onContinue: (choices: OnboardingChoices) => void;
}) {
  const caps = useHost((s) => s.capabilities);
  const [touched, setTouched] = useState<
    Partial<Record<SettingsModule, boolean>>
  >({});
  const [style, setStyle] = useState<TrayStyle>("combined");
  const [launchAtLogin, setLaunchAtLogin] = useState(true);

  const defaults = defaultEnabled(caps);
  const items: ModuleToggleItem[] = SETTINGS_MODULES.map((m) => ({
    id: m,
    ...MODULES[m],
    available: modulePresence(caps?.modules[m], caps !== null) === "present",
    enabled: touched[m] ?? defaults[m],
  }));
  const status = chipStatus(hostInfo);
  const enabledNow = Object.fromEntries(
    items.map((i) => [i.id, i.available && i.enabled])
  ) as Record<SettingsModule, boolean>;

  const submit = () => {
    const enabled: OnboardingChoices["enabled"] = {};
    for (const item of items) {
      if (item.available) enabled[item.id as SettingsModule] = item.enabled;
    }
    onContinue({ enabled, style, launchAtLogin });
  };

  return (
    <OnboardingFrame
      step={1}
      title="Set up Kelvo"
      sub="Pick what to sample and how it shows in the menu bar. Everything here is in Settings later."
      footer={
        <>
          <Checkbox
            id="onboarding-login"
            checked={launchAtLogin}
            onCheckedChange={(v) => setLaunchAtLogin(v === true)}
          />
          <label
            htmlFor="onboarding-login"
            className="font-normal text-[13px] text-fg-subtle"
          >
            Launch at login
          </label>
          <span className="flex-1" />
          {status && (
            <span
              className={cn(
                "figures text-[11px]",
                hostInfo?.chip_known ? "text-fg-faint" : "text-muted-foreground"
              )}
            >
              {status}
            </span>
          )}
          <Button variant="outline" disabled={busy} onClick={onSkip}>
            Skip
          </Button>
          <Button disabled={busy} onClick={submit}>
            Continue
          </Button>
        </>
      }
    >
      <div className="grid grid-cols-2 gap-6">
        <div className="flex flex-col gap-2">
          <span className={FIELD_LABEL}>Modules</span>
          <ModuleToggleList
            modules={items}
            onToggle={(id, on) =>
              setTouched((t) => ({ ...t, [id as SettingsModule]: on }))
            }
          />
        </div>
        <TrayStyleGroup
          value={style}
          onValueChange={setStyle}
          aria-labelledby="onboarding-style"
          className="flex flex-col gap-2"
        >
          <span id="onboarding-style" className={FIELD_LABEL}>
            Menu bar style
          </span>
          <TrayStyleCards enabled={enabledNow} />
        </TrayStyleGroup>
      </div>
    </OnboardingFrame>
  );
}
