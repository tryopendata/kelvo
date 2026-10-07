/**
 * Shared number rules for every formatter in this folder.
 *
 * Missing input (`null`, `undefined`, NaN, ±Infinity) renders as an em dash,
 * never as "0": a zero reads as a real measurement.
 */
export const MISSING = "—";

export type MaybeNumber = number | null | undefined;

/** A formatted figure split from its unit, for layouts that style them apart. */
export interface Quantity {
  value: string;
  unit: string;
}

export function isPresent(v: MaybeNumber): v is number {
  return v != null && Number.isFinite(v);
}

/**
 * `toFixed` with a typographic minus (U+2212) and no
 * negative zero ("-0.0" becomes "0.0").
 */
export function fixed(v: number, decimals: number): string {
  const s = v.toFixed(decimals);
  if (Number(s) === 0) return s.replace("-", "");
  return s.startsWith("-") ? `−${s.slice(1)}` : s;
}

/**
 * Figure precision: one decimal below 100, none from 100 up
 * ("6.0 GB", "38.4 MB/s", "220 MB/s", "330 GB"). Decided after rounding, so
 * 99.96 reads "100", not "100.0".
 */
export function autoDecimals(v: number): number {
  return Math.abs(Number(v.toFixed(1))) >= 100 ? 0 : 1;
}

/** "14.8 W" or, compact, "14.8W" (menu bar). */
export function joinQuantity(q: Quantity, compact = false): string {
  return compact ? `${q.value}${q.unit}` : `${q.value} ${q.unit}`;
}
