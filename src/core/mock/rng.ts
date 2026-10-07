/**
 * Seeded generator for sample data: a Lehmer RNG and a mean-reverting
 * random walk, so the mock transport's backfill is the same every run.
 */
export function rng(seed: number): () => number {
  let s = seed;
  return () => {
    s = (s * 16807) % 2147483647;
    return (s - 1) / 2147483646;
  };
}

export interface WalkSpec {
  base: number;
  noise: number;
  min?: number;
  max?: number;
}

/**
 * One step of the walk: noise, then a 20% pull back to the base.
 * Returns the unclamped state; clamp what you emit with `clampWalk`
 * (`Math.max(1, v)`).
 */
export function walkStep(v: number, r: () => number, spec: WalkSpec): number {
  const next = v + (r() - 0.5) * spec.noise;
  return next + (spec.base - next) * 0.2;
}

export function clampWalk(v: number, spec: WalkSpec): number {
  return Math.min(spec.max ?? Infinity, Math.max(spec.min ?? -Infinity, v));
}

/** `series(n, seed, base, noise)`: `n` steps of the walk from `seed`. */
export function walkSeries(n: number, seed: number, spec: WalkSpec): number[] {
  const r = rng(seed);
  let v = spec.base;
  const out: number[] = [];
  for (let i = 0; i < n; i++) {
    v = walkStep(v, r, spec);
    out.push(clampWalk(v, spec));
  }
  return out;
}
