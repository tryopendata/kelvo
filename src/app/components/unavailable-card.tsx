import {
  MODULE_NAMES,
  type ModuleState,
  type UiModule,
  unavailableText,
} from "@core/module-state";
import type { Accent } from "~/widgets/lib/accent";
import { ModuleCard } from "~/widgets/module-card";

export const MODULE_ACCENT: Record<UiModule, Accent> = {
  cpu: "cpu",
  gpu: "gpu",
  memory: "mem",
  power: "power",
  network: "net",
  disk: "disk",
  battery: "battery",
};

/**
 * A module the build cannot run here (plan 4.17): "Not available in this
 * edition" for an App Store build without the entitlement, otherwise "Not
 * available on this Mac". Same card shell as its live siblings, no figures.
 */
export function UnavailableCard({
  module,
  state,
}: {
  module: UiModule;
  state: ModuleState;
}) {
  return (
    <ModuleCard accent={MODULE_ACCENT[module]} title={MODULE_NAMES[module]}>
      <p className="m-0 font-normal text-[12px] text-muted-foreground">
        {unavailableText(state) ?? "Not available"}
      </p>
    </ModuleCard>
  );
}
