export const DOCKER_RANGES = {
  "15m": 900_000,
  "1h": 3_600_000,
  "24h": 86_400_000,
  "7d": 604_800_000,
  "30d": 2_592_000_000,
} as const;
export type DockerRange = keyof typeof DOCKER_RANGES;

/** Router strings are untrusted even in the memory router (back/restore state). */
export function dockerRange(
  raw: string | null,
  fallback: DockerRange
): DockerRange {
  return raw !== null && Object.hasOwn(DOCKER_RANGES, raw)
    ? (raw as DockerRange)
    : fallback;
}

/** Invalid/empty endpoints resume live history instead of sending NaN to Rust. */
export function historyEnd(raw: string | null, offset = 0): number | null {
  if (raw === null || raw.trim() === "") return null;
  const value = Number(raw) + offset;
  return Number.isFinite(value) ? value : null;
}
