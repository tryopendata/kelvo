/**
 * DVFS residency for the CPU and GPU pages (plan 4.7, 4.8). Residency series
 * carry a `state` label that is a frequency in MHz or `idle`. The page shows
 * the four states with the most time, highest frequency first, everything
 * else merged into "other", and idle last.
 */

export const IDLE_STATE = "idle";
export const OTHER_STATE = "other";

export interface ResidencyRow {
  /** "4.51 GHz", "other" or "idle". */
  label: string;
  /** Percent of the window. */
  pct: number;
}

/** "4512" (MHz) as "4.51 GHz". */
export function stateLabel(mhz: number): string {
  return `${(mhz / 1000).toFixed(2)} GHz`;
}

/**
 * Rows for a residency bar from each state's average percent. States with no
 * measured value are left out. "other" appears only when the merged states add
 * up to at least half a percent, so a table never shows "other 0%". Returns
 * null when no state has a value.
 */
export function residencyRows(
  pctByState: Readonly<Record<string, number | null>>,
  keep = 4
): ResidencyRow[] | null {
  let idle: number | null = null;
  const busy: { mhz: number; pct: number }[] = [];
  for (const [state, pct] of Object.entries(pctByState)) {
    if (pct == null || !Number.isFinite(pct)) continue;
    if (state === IDLE_STATE) {
      idle = pct;
      continue;
    }
    const mhz = Number(state);
    if (Number.isFinite(mhz)) busy.push({ mhz, pct });
  }
  if (idle === null && busy.length === 0) return null;

  // A state under half a percent would read "0%"; it goes to "other" instead
  // of taking one of the four rows.
  const bySize = busy
    .filter((s) => s.pct >= 0.5)
    .sort((a, b) => b.pct - a.pct || b.mhz - a.mhz);
  const kept = bySize.slice(0, keep).sort((a, b) => b.mhz - a.mhz);
  const other = busy
    .filter((s) => !kept.includes(s))
    .reduce((sum, s) => sum + s.pct, 0);

  const rows: ResidencyRow[] = kept.map((s) => ({
    label: stateLabel(s.mhz),
    pct: s.pct,
  }));
  if (other >= 0.5) rows.push({ label: OTHER_STATE, pct: other });
  if (idle !== null) rows.push({ label: IDLE_STATE, pct: idle });
  return rows;
}
