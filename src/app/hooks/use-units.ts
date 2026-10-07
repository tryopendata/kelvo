import type { ByteUnits, RateUnits, TemperatureUnits } from "@core/format";
import { useStore } from "zustand";
import { useShallow } from "zustand/react/shallow";
import { useSettingsStore } from "~/stores/settings-store";

export interface DisplayUnits {
  rate: RateUnits;
  bytes: ByteUnits;
  temperature: TemperatureUnits;
}

/**
 * The Units settings (plan 4.15) as the formatters' option values. Display
 * only; before settings arrive, the defaults (MB/s, GB, °F).
 */
export function useUnits(): DisplayUnits {
  return useStore(
    useSettingsStore(),
    useShallow((s) => {
      const u = s.snapshot?.settings.units;
      return {
        rate: u?.network === "bits_per_sec" ? "Mbps" : "MBps",
        bytes: u?.memory === "binary" ? "GiB" : "GB",
        temperature: u?.temperature === "celsius" ? "C" : "F",
      } satisfies DisplayUnits;
    })
  );
}
