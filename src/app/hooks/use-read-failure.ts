import { clockTime } from "@core/history-state";
import { readFailureMs } from "@core/read-failure";
import { useHost } from "~/stores/host-store";

/**
 * The card notice for a headline series whose collector stopped answering
 * (plan 4.17 "Sensor read failed"), or undefined while it reads fine. The
 * selector returns a primitive, so a healthy card does not re-render for it.
 */
export function useReadFailure(key: string): string | undefined {
  const lastGood = useHost((s) => readFailureMs(s, key));
  if (lastGood === undefined) return undefined;
  return lastGood === null
    ? "Sensor read failed"
    : `Sensor read failed · last value ${clockTime(lastGood)}`;
}
