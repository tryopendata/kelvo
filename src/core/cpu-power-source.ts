import { METRIC_CODES } from "@core/generated/bindings";

/**
 * What `power.cpu_source` says about `power.cpu` (D-054, D-065), as the
 * muted line shown under CPU power. The series is present only where the SMC
 * P-cluster keys stand in for PMP (the M3 Max); there `power.cpu` leaves out
 * the E cluster (no key tracks it), so every value of it says "P cores".
 * The codes are `METRIC_CODES["power.cpu_source"]`:
 *
 * - uncalibrated: no scale yet, reads about 25% low.
 * - calibrated: scaled by a window that closed this session: nothing more to say.
 * - seeded: scaled by a stored or default scale until a window closes:
 *   "estimated from last calibration".
 * - Any other value (a newer engine) is treated as uncalibrated, per the catalog.
 *
 * `null` (absent: PMP measures all clusters, or no current value) says
 * nothing.
 */
const SOURCE = METRIC_CODES["power.cpu_source"];

export function cpuPowerNote(source: number | null): string | null {
  if (source === null) return null;
  if (source === SOURCE.calibrated) return "CPU power: P cores";
  if (source === SOURCE.seeded)
    return "CPU power: P cores, estimated from last calibration";
  return "CPU power: P cores, uncalibrated";
}
