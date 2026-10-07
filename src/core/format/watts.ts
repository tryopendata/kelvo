import {
  autoDecimals,
  fixed,
  isPresent,
  joinQuantity,
  type MaybeNumber,
  MISSING,
  type Quantity,
} from "./number";

export interface WattsOptions {
  /** Default: one decimal below 100 W, none above ("14.8 W", "0.0 W"). */
  decimals?: number;
}

export function wattsParts(
  watts: number,
  { decimals }: WattsOptions = {}
): Quantity {
  return { value: fixed(watts, decimals ?? autoDecimals(watts)), unit: "W" };
}

/**
 * "14.8 W", or compact "14.8W" for the menu bar. Negative values (battery
 * discharge) keep a typographic minus.
 */
export function formatWatts(
  watts: MaybeNumber,
  options: WattsOptions & { compact?: boolean } = {}
): string {
  if (!isPresent(watts)) return MISSING;
  return joinQuantity(wattsParts(watts, options), options.compact);
}

/**
 * Small powers for per-process tables (D-093): watts from 1 W up, else
 * milliwatts ("340 mW"), and "<1 mW" for a trickle, so a background
 * process does not read "0.0 W".
 */
export function formatWattsFine(watts: MaybeNumber): string {
  if (!isPresent(watts)) return MISSING;
  if (Math.abs(watts) >= 1) return formatWatts(watts);
  const mw = watts * 1000;
  if (mw === 0) return "0 mW";
  if (Math.abs(mw) < 1) return "<1 mW";
  return `${Math.round(mw)} mW`;
}

/**
 * Energy from joules, in watt-hours (what battery capacity is quoted in):
 * "1.24 Wh" from 1 Wh, "86 mWh" below, "<1 mWh" for a trickle.
 */
export function formatEnergy(joules: MaybeNumber): string {
  if (!isPresent(joules)) return MISSING;
  const wh = joules / 3600;
  if (Math.abs(wh) >= 100) return `${fixed(wh, 0)} Wh`;
  if (Math.abs(wh) >= 1) return `${fixed(wh, 2)} Wh`;
  const mwh = wh * 1000;
  if (mwh === 0) return "0 mWh";
  if (Math.abs(mwh) < 1) return "<1 mWh";
  return `${Math.round(mwh)} mWh`;
}
