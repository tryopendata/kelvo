import { describe, expect, it } from "vitest";
import { fittedCeiling } from "./nice-ceiling";

describe("fitted window ceiling", () => {
  it.each([0.13, 0.17, 1.3, 1.7, 2.01, 19.9, 59, 170, 220_000_000])(
    "keeps the visible peak %s within 5–15% headroom",
    (maximum) => {
      const ceiling = fittedCeiling(maximum, 0.1);
      expect(ceiling).toBeGreaterThanOrEqual(maximum * 1.05 - 1e-12);
      expect(ceiling).toBeLessThanOrEqual(maximum * 1.15);
    }
  );

  it.each([0, Number.NaN, Number.POSITIVE_INFINITY])(
    "uses the idle floor for %s instead of an invalid axis",
    (maximum) => {
      expect(fittedCeiling(maximum, 0.1)).toBe(0.1);
    }
  );
});
