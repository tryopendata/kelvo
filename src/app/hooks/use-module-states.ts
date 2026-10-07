import {
  type ModuleState,
  moduleState,
  UI_MODULES,
  type UiModule,
} from "@core/module-state";
import { useMemo } from "react";
import { useHost } from "~/stores/host-store";
import { useSettings } from "~/stores/settings-store";

/**
 * Each UI module's state on the current host, from live capabilities and the
 * settings mirror (plan 4.17). Both inputs keep their identity between
 * frames, so a tick does not recompute it.
 */
export function useModuleStates(): Record<UiModule, ModuleState> {
  const caps = useHost((s) => s.capabilities);
  const modules = useSettings((s) => s.modules);
  return useMemo(() => {
    const out = {} as Record<UiModule, ModuleState>;
    for (const m of UI_MODULES) out[m] = moduleState(caps, modules, m);
    return out;
  }, [caps, modules]);
}
