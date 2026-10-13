import type { LiveProcess } from "./generated/bindings";

/** Identity and last observed eligibility, never a name-based kill pattern. */
export type ProcessTarget = Pick<
  LiveProcess,
  "pid" | "start_time_us" | "name" | "refusal"
> & { running?: boolean };

export const targetKey = (
  p: Pick<ProcessTarget, "pid" | "start_time_us">
): string => `${p.pid}:${p.start_time_us}`;

export interface SelectableProcessRow {
  key: string;
  label: string;
  targets: readonly ProcessTarget[];
  parentKey?: string;
}

/** Hold siblings in place while keeping newly expanded children under their app. */
export function holdProcessRows(
  rows: readonly SelectableProcessRow[],
  prior: readonly string[]
): SelectableProcessRow[] {
  const byKey = new Map(rows.map((r) => [r.key, r]));
  const previous = new Set(prior);
  const keys = [
    ...prior.filter((key) => byKey.has(key)),
    ...rows.filter((r) => !previous.has(r.key)).map((r) => r.key),
  ];
  const ordered = keys.flatMap((key) => {
    const row = byKey.get(key);
    return row ? [row] : [];
  });
  const children = new Map<string, SelectableProcessRow[]>();
  for (const row of ordered) {
    if (row.parentKey) {
      const group = children.get(row.parentKey) ?? [];
      group.push(row);
      children.set(row.parentKey, group);
    }
  }
  return ordered
    .filter((r) => !r.parentKey)
    .flatMap((r) => [r, ...(children.get(r.key) ?? [])]);
}

export function uniqueTargets(
  targets: readonly ProcessTarget[]
): ProcessTarget[] {
  return [...new Map(targets.map((p) => [targetKey(p), p])).values()];
}

export interface ProcessSelection {
  targets: readonly ProcessTarget[];
  anchor: string | null;
}

export function selectProcessRow(
  state: ProcessSelection,
  rows: readonly SelectableProcessRow[],
  key: string,
  {
    range = false,
    additive = false,
    toggle = false,
  }: { range?: boolean; additive?: boolean; toggle?: boolean } = {}
): ProcessSelection {
  const index = rows.findIndex((r) => r.key === key);
  if (index < 0) return state;
  const row = rows[index];
  if (!row) return state;
  const anchor = rows.findIndex((r) => r.key === state.anchor);
  const targets = uniqueTargets(
    (range && anchor >= 0
      ? rows
          .slice(Math.min(anchor, index), Math.max(anchor, index) + 1)
          .flatMap((r) => r.targets)
      : row.targets
    ).filter((p) => p.running !== false)
  );
  const previous = new Map(state.targets.map((p) => [targetKey(p), p]));
  const allSelected =
    targets.length > 0 && targets.every((p) => previous.has(targetKey(p)));
  const next = additive || toggle ? previous : new Map<string, ProcessTarget>();
  for (const p of targets) {
    if (toggle && !range && allSelected) next.delete(targetKey(p));
    else next.set(targetKey(p), p);
  }
  return {
    targets: [...next.values()],
    anchor: range && anchor >= 0 ? state.anchor : key,
  };
}
