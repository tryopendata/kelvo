import type { CSSProperties } from "react";

/**
 * Module accent tokens (design-system.md "Module accents"). Each maps to
 * `--color-<accent>` (fill) and `--color-<accent>-ink` (lines, text, thin
 * marks; darker in light mode).
 */
export type Accent =
  | "cpu"
  | "cpu-2"
  | "gpu"
  | "mem"
  | "power"
  | "temp"
  | "net"
  | "disk"
  | "battery";

/** Ramp step of the card accent: 100%, 60%, 35%, 20%, or a diagonal hatch. */
export type RampStep = 1 | 2 | 3 | 4 | "hatch";

/** Corner the card glow comes from. CardGrid assigns these by index. */
export type Corner = "tl" | "tr" | "bl" | "br";

export const CORNER_ORIGIN: Record<Corner, string> = {
  tl: "0% 0%",
  tr: "100% 0%",
  bl: "0% 100%",
  br: "100% 100%",
};

/**
 * CSS custom properties that put an accent in scope: `--a` (fill) and
 * `--a-ink` (strokes and text). Children read them with `var(--a)`.
 */
export function accentVars(accent: Accent, origin?: Corner): CSSProperties {
  const vars: Record<string, string> = {
    "--a": `var(--color-${accent})`,
    "--a-ink": `var(--color-${accent}-ink)`,
  };
  if (origin) vars["--o"] = CORNER_ORIGIN[origin];
  return vars as CSSProperties;
}

const RAMP_PCT: Record<1 | 2 | 3 | 4, string> = {
  1: "var(--ramp-1)",
  2: "var(--ramp-2)",
  3: "var(--ramp-3)",
  4: "var(--ramp-4)",
};

/**
 * Background for a ramp step of the accent in scope (`--a`). The hatch is a
 * diagonal stripe in the accent so a zero-width slice still has a legend
 * swatch that matches.
 */
export function rampBackground(step: RampStep, color = "var(--a)"): string {
  if (step === "hatch") {
    return `repeating-linear-gradient(135deg, ${color} 0 1.5px, transparent 1.5px 4px)`;
  }
  if (step === 1) return color;
  return `color-mix(in srgb, ${color} ${RAMP_PCT[step]}, transparent)`;
}

/** Stroke or fill color for a ramp step, for SVG marks. */
export function rampColor(step: 1 | 2 | 3 | 4, color = "var(--a)"): string {
  if (step === 1) return color;
  return `color-mix(in srgb, ${color} ${RAMP_PCT[step]}, transparent)`;
}
