import { describe, expect, it } from "vitest";
import { formatBytes, formatRate } from "./bytes";
import { figureAt, parseFigure, tickPath } from "./figure";
import { formatPercent } from "./percent";

const at = (from: string, to: string, t: number) => {
  const a = parseFigure(from);
  const b = parseFigure(to);
  const path = tickPath(a, b);
  if (!b || !path) throw new Error("no path");
  const s = path.from + (path.to - path.from) * t;
  return figureAt(b, path.log ? Math.exp(s) : s);
};

describe("parseFigure", () => {
  it("reads unit ladders into base units", () => {
    expect(parseFigure("10 GB")).toMatchObject({ base: 10e9, rung: 3 });
    expect(parseFigure("100 MB")).toMatchObject({ base: 100e6, rung: 2 });
    expect(parseFigure("1.5 GiB")?.base).toBe(1.5 * 1024 ** 3);
    expect(parseFigure("38.4 MB/s")?.base).toBeCloseTo(38.4e6);
  });

  it("keeps a plain suffix, prefix, precision, sign and grouping", () => {
    expect(parseFigure("12.4%")).toMatchObject({
      base: 12.4,
      suffix: "%",
      decimals: 1,
      ladder: -1,
    });
    expect(parseFigure("+18k")).toMatchObject({ prefix: "+", base: 18 });
    expect(parseFigure("−3.5")?.base).toBe(-3.5);
    expect(parseFigure("1,840")).toMatchObject({ base: 1840, grouped: true });
  });

  it("returns null for text that isn't a figure", () => {
    expect(parseFigure("—")).toBeNull();
    expect(parseFigure("Idle")).toBeNull();
  });
});

describe("figureAt", () => {
  it("writes the formatter's own output at the end values", () => {
    for (const text of [formatBytes(10e9), formatBytes(840e6)]) {
      const f = parseFigure(text);
      if (f) expect(figureAt(f, f.base)).toBe(text);
    }
    const r = parseFigure(formatRate(38.4e6));
    if (r) expect(figureAt(r, r.base)).toBe("38.4 MB/s");
    expect(figureAt(parseFigure("1,840") as never, 1203.4)).toBe("1,203");
  });
});

describe("tickPath", () => {
  it("counts down across units, through each unit on the way", () => {
    expect(at("10 GB", "100 MB", 0)).toBe("10.0 GB");
    expect(at("10 GB", "100 MB", 0.5)).toBe("1.0 GB");
    expect(at("10 GB", "100 MB", 0.75)).toBe("316 MB");
    expect(at("10 GB", "100 MB", 1)).toBe("100 MB");
    expect(at("10 MB", "100 MB", 0.5)).toBe("31.6 MB");
  });

  it("counts a percentage linearly at its own precision", () => {
    expect(at(formatPercent(20), formatPercent(60), 0.5)).toBe("40%");
  });

  it("replaces small changes, unlike kinds and missing values in place", () => {
    const p = (a: string, b: string) =>
      tickPath(parseFigure(a), parseFigure(b));
    expect(p("40%", "42%")).toBeNull(); // under 10%
    expect(p("5%", "6%")).toBeNull(); // under three display steps
    expect(p("17.6 GB", "17.7 GB")).toBeNull();
    expect(p("40%", "40 W")).toBeNull();
    expect(p("10 GB", "10 MB/s")).toBeNull();
    expect(p("—", "40%")).toBeNull();
    expect(p("0 B/s", "40.0 MB/s")).toBeNull();
    expect(p("999 MB", "1.0 GB")).not.toBeNull(); // unit change
  });
});
