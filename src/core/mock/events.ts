/**
 * `query_events` for the mock transport: a fixed day of detector and alert
 * events relative to `nowMs`. With the mock clock (Sun Oct 4, 22:40) the
 * fan ramp lands at 14:02 and the ANE spike 6.6 minutes before now, where
 * the power chart marks it.
 */
import type { Event } from "@core/generated/bindings";

const SEC = 1000;
const MIN = 60 * SEC;
const HOUR = 60 * MIN;

export function mockEvents(nowMs: number): Event[] {
  const at = (agoMs: number) => nowMs - agoMs;
  return [
    {
      ts_ms: at(12 * HOUR + 28 * MIN),
      start_ms: at(12 * HOUR + 31 * MIN),
      processes: ["com.docker.backend"],
      detail: {
        kind: "sustained_process",
        process: "com.docker.backend",
        cpu_pct: 142,
        secs: 180,
      },
    },
    {
      ts_ms: at(8 * HOUR + 38 * MIN),
      start_ms: at(8 * HOUR + 39 * MIN),
      processes: ["kernel_task", "Xcode build"],
      detail: { kind: "fans_ramped", from_rpm: 1450, to_rpm: 4210 },
    },
    {
      ts_ms: at(3 * HOUR + 20 * MIN),
      start_ms: at(3 * HOUR + 25 * MIN),
      processes: ["ffmpeg"],
      detail: {
        kind: "alert",
        rule_id: "6b656c76-6f00-0000-0000-000000000001",
        rule_name: "Process above 200% CPU for 5 minutes",
        cause: { type: "process_cpu", process: "ffmpeg", cpu_pct: 251 },
      },
    },
    {
      ts_ms: at(6 * MIN + 36 * SEC),
      start_ms: at(6 * MIN + 50 * SEC),
      processes: ["Photos face analysis"],
      detail: {
        kind: "power_spike",
        component: "ane",
        watts: 1.4,
        baseline_watts: 0.1,
      },
    },
  ];
}
