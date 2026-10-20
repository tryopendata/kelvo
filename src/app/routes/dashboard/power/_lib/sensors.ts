/**
 * Pure helpers for the Power & Sensors page (plan 4.10).
 */
import { MISSING, rpmParts } from "@core/format";

/** "Zone 01" for the first zone in layout order. */
export function zoneName(index: number): string {
  return `Zone ${String(index + 1).padStart(2, "0")}`;
}

/** `thermal.sensor{name}` display label. */
export function sensorLabel(name: string): string {
  switch (name.toLowerCase()) {
    case "battery":
      return "Battery";
    case "ssd":
    case "ssd (nand)":
      return "SSD (NAND)";
    case "wifi":
    case "wi-fi module":
      return "Wi‑Fi module";
    default:
      return name;
  }
}

/** "Left 1,840 · right 1,860" for two fans; "Fan 1,850" for one. */
export function fanSummary(fans: readonly (number | null)[]): string {
  const f = (v: number | null) => (v === null ? MISSING : rpmParts(v).value);
  if (fans.length === 1) return `Fan ${f(fans[0] ?? null)}`;
  if (fans.length === 2)
    return `Left ${f(fans[0] ?? null)} · right ${f(fans[1] ?? null)}`;
  return fans.map((v, i) => `${i + 1}: ${f(v)}`).join(" · ");
}

/**
 * `fan.mode` as a word. The SMC `F%dMd` encoding is unverified (catalog 6.1);
 * 0 is taken as automatic and 1 as forced, anything else is not shown.
 */
export function fanModeLabel(mode: number | null): string {
  if (mode === 0) return "Automatic";
  if (mode === 1) return "Manual";
  return "—";
}
