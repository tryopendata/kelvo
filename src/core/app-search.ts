/**
 * The process search shared by the Processes table and the per-app usage
 * tables (plan 4.14, D-099): a case-insensitive substring of the name, or a
 * PID or listening port that starts with the digits typed.
 */

/** What a search looks at. An app row has a name only. */
export interface ProcessQueryTarget {
  name: string;
  pid?: number;
  ports?: readonly number[] | null;
}

/** A predicate for `query`, or null when it is blank (every row matches). */
export function matchProcessQuery(
  query: string
): ((p: ProcessQueryTarget) => boolean) | null {
  const q = query.trim().toLowerCase();
  if (!q) return null;
  const digits = /^\d+$/.test(q);
  const startsWith = (n: number) => String(n).startsWith(q);
  return (p) =>
    p.name.toLowerCase().includes(q) ||
    (digits &&
      ((p.pid !== undefined && startsWith(p.pid)) ||
        (p.ports ?? []).some(startsWith)));
}
