import {
  autoDecimals,
  fixed,
  isPresent,
  joinQuantity,
  type MaybeNumber,
  MISSING,
  type Quantity,
} from "./number";

/** The `units.memory` setting: decimal GB (10^9) or binary GiB (2^30). */
export type ByteUnits = "GB" | "GiB";

/** The `units.network` setting: bytes per second or bits per second. */
export type RateUnits = "MBps" | "Mbps";

const DECIMAL = ["B", "KB", "MB", "GB", "TB", "PB"] as const;
const BINARY = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"] as const;
const BYTE_RATE = ["B/s", "KB/s", "MB/s", "GB/s", "TB/s"] as const;
const BIT_RATE = ["b/s", "Kb/s", "Mb/s", "Gb/s", "Tb/s"] as const;

/** Every unit ladder `scale` walks, with its step. */
export const UNIT_LADDERS: readonly {
  units: readonly string[];
  step: number;
}[] = [
  { units: DECIMAL, step: 1000 },
  { units: BINARY, step: 1024 },
  { units: BYTE_RATE, step: 1000 },
  { units: BIT_RATE, step: 1000 },
];

export type ByteUnit = (typeof DECIMAL)[number] | (typeof BINARY)[number];
export type RateUnit = (typeof BYTE_RATE)[number] | (typeof BIT_RATE)[number];

/**
 * Scale `v` up the unit ladder until it shows at most three integer digits,
 * or to `pinned` when the caller lines up a column in one unit ("0.7 MB/s"
 * next to "22.1 MB/s"). Base units (B, B/s) never take decimals.
 */
export function scale(
  v: number,
  units: readonly string[],
  step: number,
  pinned: string | undefined,
  decimals: number | undefined
): Quantity {
  let i = 0;
  let scaled = v;
  if (pinned !== undefined && units.includes(pinned)) {
    i = units.indexOf(pinned);
    scaled = v / step ** i;
  } else {
    // 999.5 and up rounds to "1000" at zero decimals: promote instead.
    while (Math.abs(scaled) >= 999.5 && i < units.length - 1) {
      scaled /= step;
      i += 1;
    }
  }
  const d = decimals ?? (i === 0 ? 0 : autoDecimals(scaled));
  return { value: fixed(scaled, d), unit: units[i] as string };
}

export interface BytesOptions {
  /** Default "GB" (decimal). */
  units?: ByteUnits;
  /** Force one unit, e.g. "MB" for an aligned column. */
  unit?: ByteUnit;
  decimals?: number;
}

export function bytesParts(
  bytes: number,
  { units = "GB", unit, decimals }: BytesOptions = {}
): Quantity {
  return units === "GiB"
    ? scale(bytes, BINARY, 1024, unit, decimals)
    : scale(bytes, DECIMAL, 1000, unit, decimals);
}

/** "17.6 GB", "840 MB", "16.4 GiB". */
export function formatBytes(
  bytes: MaybeNumber,
  options: BytesOptions = {}
): string {
  if (!isPresent(bytes)) return MISSING;
  return joinQuantity(bytesParts(bytes, options));
}

export interface RateOptions {
  /** Default "MBps". */
  units?: RateUnits;
  /** Force one unit, e.g. "MB/s" for a per-process column. */
  unit?: RateUnit;
  decimals?: number;
}

/** Input is bytes per second, the catalog unit of `net.*` and `disk.*`. */
export function rateParts(
  bytesPerSecond: number,
  { units = "MBps", unit, decimals }: RateOptions = {}
): Quantity {
  return units === "Mbps"
    ? scale(bytesPerSecond * 8, BIT_RATE, 1000, unit, decimals)
    : scale(bytesPerSecond, BYTE_RATE, 1000, unit, decimals);
}

/** "38.4 MB/s" or, in bits, "307 Mb/s". */
export function formatRate(
  bytesPerSecond: MaybeNumber,
  options: RateOptions = {}
): string {
  if (!isPresent(bytesPerSecond)) return MISSING;
  return joinQuantity(rateParts(bytesPerSecond, options));
}
