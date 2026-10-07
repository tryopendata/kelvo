/**
 * The sum of a stack's parts, `null` when any part is missing: a stack never
 * shows a partial sum as if it were the whole.
 */
export function stackSum(
  parts: readonly (number | null | undefined)[]
): number | null {
  let sum = 0;
  for (const v of parts) {
    if (v == null) return null;
    sum += v;
  }
  return sum;
}

/**
 * What a whole leaves after its stacked parts, floored at 0 (power's "rest of
 * system"). `null` when the whole or any part is missing: the remainder is
 * unknown then, not that part's share added to it.
 */
export function stackRemainder(
  whole: number | null,
  parts: readonly (number | null | undefined)[]
): number | null {
  const sum = stackSum(parts);
  return whole === null || sum === null ? null : Math.max(0, whole - sum);
}
