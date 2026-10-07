import {
  fixed,
  isPresent,
  type MaybeNumber,
  MISSING,
  type Quantity,
} from "./number";

/** The `units.temperature` setting. Sensors always report °C. */
export type TemperatureUnits = "C" | "F";

export interface TemperatureOptions {
  /** Default "C". */
  units?: TemperatureUnits;
  /** Default 0 ("61 °C"). */
  decimals?: number;
}

export function celsiusToFahrenheit(celsius: number): number {
  return (celsius * 9) / 5 + 32;
}

export function temperatureParts(
  celsius: number,
  { units = "C", decimals = 0 }: TemperatureOptions = {}
): Quantity {
  const v = units === "F" ? celsiusToFahrenheit(celsius) : celsius;
  return { value: fixed(v, decimals), unit: `°${units}` };
}

/**
 * "61 °C" (a space before the degree sign). `compact`
 * is the menu bar form, "61°", which drops the scale letter.
 */
export function formatTemperature(
  celsius: MaybeNumber,
  options: TemperatureOptions & { compact?: boolean } = {}
): string {
  if (!isPresent(celsius)) return MISSING;
  const q = temperatureParts(celsius, options);
  return options.compact ? `${q.value}°` : `${q.value} ${q.unit}`;
}
