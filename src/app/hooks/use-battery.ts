import {
  type BatteryHour,
  batteryBars,
  batteryHourStarts,
} from "@core/battery-hours";
import { historyKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { useTransport } from "~/lib/transport-context";
import { type HostLiveState, useHost, useHostId } from "~/stores/host-store";

const held = (s: HostLiveState, key: string) => s.held[key] ?? null;

/**
 * Every battery series' current value (catalog 6.1), for the Battery page and
 * the Power & Sensors battery section. `null` is no current value, never 0.
 */
export const selectBatteryDetail = (s: HostLiveState) => ({
  charge: held(s, "battery.charge"),
  charging: held(s, "battery.charging"),
  external: held(s, "battery.external"),
  timeRemainingMin: held(s, "battery.time_remaining"),
  health: held(s, "battery.health"),
  cycles: held(s, "battery.cycles"),
  capacityWh: held(s, "battery.capacity_wh"),
  designWh: held(s, "battery.design_wh"),
  powerW: held(s, "battery.power"),
  tempC: held(s, "battery.temp"),
  systemW: held(s, "power.system"),
});

export type BatteryDetail = ReturnType<typeof selectBatteryDetail>;

export function useBatteryDetail(): BatteryDetail {
  return useHost(useShallow(selectBatteryDetail));
}

/**
 * The bars' local hour boundaries, moving on when the current hour closes.
 */
function useHourStarts(): number[] {
  const [starts, setStarts] = useState(() => batteryHourStarts(new Date()));
  const end = starts[starts.length - 1] as number;
  useEffect(() => {
    const t = setTimeout(
      () => setStarts(batteryHourStarts(new Date())),
      Math.max(1000, end - Date.now() + 1000)
    );
    return () => clearTimeout(t);
  }, [end]);
  return starts;
}

/**
 * How often the bars refresh: Rust answers through now (D-092), and a bar is
 * its hour's last minute, so it can move once a minute.
 */
const BATTERY_REFRESH_MS = 60_000;

/**
 * Hourly charge bars for the last 24 local hours (plan 4.10) from
 * `battery_hours`: on mount, when an hour closes, and every minute.
 */
export function useBatteryHours(): {
  hours: BatteryHour[] | undefined;
  error: Error | null;
} {
  const transport = useTransport();
  const hostId = useHostId();
  const starts = useHourStarts();
  const { data, error } = useQuery({
    queryKey: historyKeys.batteryHours(hostId, starts[starts.length - 1] ?? 0),
    queryFn: async () =>
      batteryBars(await unwrap(transport.batteryHours(hostId, starts))),
    staleTime: BATTERY_REFRESH_MS,
    refetchInterval: BATTERY_REFRESH_MS,
  });
  return { hours: data, error };
}
