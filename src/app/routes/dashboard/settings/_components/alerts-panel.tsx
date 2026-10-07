import type { AlertSettings } from "@core/generated/bindings";
import { SettingsRow } from "~/components/settings-row";
import { Switch } from "~/components/ui/switch";
import { useWriteSettings } from "~/hooks/use-write-settings";

const OFF: AlertSettings = { hot_process: false, thermal_serious: false };

/**
 * The two built-in alert rules (v1.2, D-084), as switch rows. Both
 * start off. Switching one on posts an "Alerts are on" notification right
 * away, because macOS asks for notification permission on a first delivery,
 * not before (D-084). A fired alert is a notification and a Timeline event,
 * at most once every 30 minutes per rule.
 */
export function AlertsPanel({ alerts = OFF }: { alerts?: AlertSettings }) {
  const write = useWriteSettings();
  return (
    <section
      aria-labelledby="settings-alerts"
      className="flex flex-col gap-2.5"
    >
      <h2 id="settings-alerts" className="font-[590] text-[14px]">
        Alerts
      </h2>
      <div className="overflow-hidden rounded-card border border-border bg-card">
        <SettingsRow
          label="A process above 200% CPU for 5 minutes"
          sub="Any one process, for the whole 5 minutes"
        >
          <Switch
            checked={alerts.hot_process}
            aria-label="Alert when a process is above 200% CPU for 5 minutes"
            onCheckedChange={(on) => write({ alerts: { hot_process: on } })}
          />
        </SettingsRow>
        <SettingsRow
          label="Thermal state Serious or worse"
          sub="macOS is slowing the Mac down to cool it"
        >
          <Switch
            checked={alerts.thermal_serious}
            aria-label="Alert when the thermal state is Serious or worse"
            onCheckedChange={(on) => write({ alerts: { thermal_serious: on } })}
          />
        </SettingsRow>
      </div>
      <p className="m-0 text-[11px] text-muted-foreground">
        Sent as a notification, at most once every 30 minutes each, and marked
        on the Timeline. Switching one on sends a test notification, so macOS
        can ask for permission now.
      </p>
    </section>
  );
}
