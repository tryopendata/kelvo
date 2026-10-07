import {
  isPresent,
  joinQuantity,
  type MaybeNumber,
  MISSING,
  type Quantity,
} from "./number";

const grouped = new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 });

/** Fan speed split from its unit: { value: "1,850", unit: "RPM" }. */
export function rpmParts(rpm: number): Quantity {
  return { value: grouped.format(rpm), unit: "RPM" };
}

/** Fan speed, rounded and grouped: "1,850 RPM". */
export function formatRpm(rpm: MaybeNumber): string {
  if (!isPresent(rpm)) return MISSING;
  return joinQuantity(rpmParts(rpm));
}
