import { clamp01 } from "./numeric";

export interface RingArc {
  /** Visible arc length in user units. */
  length: number;
  /** `stroke-dasharray`: arc length then the full circumference. */
  dasharray: string;
  /** `stroke-dashoffset`: negative start, so segments follow each other. */
  dashoffset: number;
}

export interface RingGeometry {
  circumference: number;
  arcs: RingArc[];
}

export function ringCircumference(radius: number): number {
  return 2 * Math.PI * radius;
}

/**
 * Dash geometry for a ring gauge drawn as stacked `<circle>` strokes, as in
 * the Overview CPU ring (r=30, C=188.5, "User" then "System" starting where
 * "User" ends). Fractions are clamped to [0, 1] and their running total to 1,
 * so segments never overlap or wrap past the start. Lengths are rounded to
 * 0.1.
 *
 * Rotate the group -90° so the arc starts at 12 o'clock.
 */
export function ringArcs(
  radius: number,
  fractions: readonly number[]
): RingGeometry {
  const circumference = ringCircumference(radius);
  const c = circumference.toFixed(1);
  let used = 0;
  const arcs = fractions.map((f) => {
    const clamped = clamp01(f);
    const take = Math.min(clamped, 1 - used);
    const start = used;
    used += take;
    const length = round1(take * circumference);
    return {
      length,
      dasharray: `${length.toFixed(1)} ${c}`,
      dashoffset: start === 0 ? 0 : -round1(start * circumference),
    };
  });
  return { circumference, arcs };
}

function round1(v: number): number {
  return Math.round(v * 10) / 10;
}
