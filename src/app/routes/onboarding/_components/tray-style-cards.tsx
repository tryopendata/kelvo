import { type SettingsModule, trayStyleMenuBar } from "@core/settings-patch";
import { trayLayout } from "@core/tray-layout";
import { TrayStyleOption } from "~/components/tray-style-option";
import { useTrayReadings, useTrayUnits } from "~/hooks/use-tray-readings";

/**
 * The three menu bar style cards, each previewing its preset over the live
 * values. It owns the tick subscription, so a tick re-renders the cards, not
 * the whole setup step.
 */
export function TrayStyleCards({
  enabled,
}: {
  enabled: Record<SettingsModule, boolean>;
}) {
  const readings = useTrayReadings();
  const units = useTrayUnits();
  const layout = (style: Parameters<typeof trayStyleMenuBar>[0]) =>
    trayLayout(trayStyleMenuBar(style), (m) => enabled[m], readings, units);
  return (
    <>
      <TrayStyleOption
        style="combined"
        title="Combined"
        description="One item. Smallest footprint."
        recommended
        layout={layout("combined")}
      />
      <TrayStyleOption
        style="graphs"
        title="Graph per module"
        description="Separate items you can reorder with ⌘-drag."
        layout={layout("graphs")}
      />
      <TrayStyleOption
        style="values"
        title="Values only"
        description="Numbers with stacked labels."
        layout={layout("values")}
      />
    </>
  );
}
