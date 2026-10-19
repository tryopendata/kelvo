import type { SpaceNode } from "@core/generated/bindings";
import { describe, expect, it } from "vitest";
import {
  layoutSpace,
  spaceEntries,
  spaceOrder,
  tileLabel,
  UNMEASURED_KEY,
} from "./space-treemap";

const node = (id: number, bytes: number, done = true): SpaceNode => ({
  id,
  name: `n${id}`,
  kind: "folder",
  bytes,
  items: 1,
  done,
  openable: true,
});

describe("spaceEntries", () => {
  it("sorts a finished folder largest first and drops empty entries", () => {
    const entries = spaceEntries([node(1, 5), node(2, 0), node(3, 9)], null);
    expect(entries.map((e) => e.key)).toEqual(["3", "1"]);
  });

  it("keeps finish order while scanning and draws the rest as unmeasured", () => {
    const first = spaceEntries([node(1, 5), node(2, 3, false)], 95);
    expect(first.map((e) => e.key)).toEqual(["1", UNMEASURED_KEY]);
    const next = spaceEntries(
      [node(1, 5), node(2, 40), node(3, 2)],
      53,
      spaceOrder(first)
    );
    expect(next.map((e) => e.key)).toEqual(["1", "2", "3", UNMEASURED_KEY]);
  });
});

describe("layoutSpace", () => {
  it("fills the box with areas proportional to bytes, minus gutters", () => {
    const tiles = layoutSpace(
      spaceEntries([node(1, 60), node(2, 30), node(3, 10)], null),
      400,
      200
    );
    expect(tiles).toHaveLength(3);
    const area = tiles.reduce((a, t) => a + t.w * t.h, 0);
    expect(area).toBeGreaterThan(400 * 200 * 0.97);
    expect(area).toBeLessThanOrEqual(400 * 200);
    const big = tiles.find((t) => t.key === "1");
    expect(big?.share).toBeCloseTo(0.6);
    expect(((big?.w ?? 0) * (big?.h ?? 0)) / area).toBeCloseTo(0.6, 1);
    for (const t of tiles) {
      expect(Number.isInteger(t.x) && Number.isInteger(t.w)).toBe(true);
    }
  });

  it("keeps finished tiles in place while a scan appends", () => {
    const at = (children: SpaceNode[], unmeasured: number, order: string[]) =>
      layoutSpace(spaceEntries(children, unmeasured, order), 800, 360);
    const before = at([node(1, 300)], 700, []);
    const after = at([node(1, 300), node(2, 500)], 200, ["1"]);
    const tile = (tiles: typeof before) => tiles.find((t) => t.key === "1");
    expect(tile(after)).toMatchObject({
      x: tile(before)?.x,
      y: tile(before)?.y,
      w: tile(before)?.w,
      h: tile(before)?.h,
    });
  });

  it("draws nothing for an empty folder or box", () => {
    expect(layoutSpace([], 100, 100)).toEqual([]);
    expect(layoutSpace(spaceEntries([node(1, 5)], null), 0, 100)).toEqual([]);
  });
});

describe("tileLabel", () => {
  it("shows what fits", () => {
    expect(tileLabel(120, 60)).toBe("full");
    expect(tileLabel(60, 24)).toBe("name");
    expect(tileLabel(20, 60)).toBe("none");
  });
});
