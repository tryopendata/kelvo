import type { Labels, SeriesKey } from "@core/generated/bindings";

/**
 * Display form of a series key, `cpu.load{core=P0}`, the same text Rust's
 * `Display` writes. Labels arrive sorted by key (bindings.ts `Labels`), so
 * equal keys always produce equal strings. Used as the map key in the live
 * store.
 */
export function seriesKeyString(key: SeriesKey): string {
  if (key.labels.length === 0) return key.metric;
  return `${key.metric}{${key.labels.map(([k, v]) => `${k}=${v}`).join(",")}}`;
}

/** Build a key from a metric and label pairs, sorting labels by key. */
export function seriesKey(
  metric: string,
  labels: Record<string, string> = {}
): SeriesKey {
  const pairs: Labels = Object.entries(labels).sort(([a], [b]) =>
    a < b ? -1 : a > b ? 1 : 0
  );
  return { metric, labels: pairs };
}

/** `seriesKeyString(seriesKey(metric, labels))`. */
export function sk(metric: string, labels: Record<string, string> = {}) {
  return seriesKeyString(seriesKey(metric, labels));
}

/** The value of label `name` on `key`, or undefined. */
export function labelOf(key: SeriesKey, name: string): string | undefined {
  return key.labels.find(([k]) => k === name)?.[1];
}

/** The `name` label of every series of `metric`, in layout order. */
export function labelValues(
  series: readonly SeriesKey[],
  metric: string,
  name: string
): string[] {
  const out: string[] = [];
  for (const s of series) {
    if (s.metric !== metric) continue;
    const v = labelOf(s, name);
    if (v !== undefined) out.push(v);
  }
  return out;
}
