import type { SpaceNode } from "@core/generated/bindings";
import { describe, expect, it } from "vitest";
import {
  FRAME_HEADER,
  framePreview,
  layoutSpace,
  spaceEntries,
  spaceOrder,
  tileLabel,
  UNMEASURED_KEY,
} from "./space-treemap";

const node = (
  id: number,
  bytes: number,
  done = true,
  children: SpaceNode[] = []
): SpaceNode => ({
  id,
  name: `n${id}`,
  kind: "folder",
  bytes,
  items: 1,
  done,
  openable: true,
  children,
});
const tileAt = <T extends { key: string }>(tiles: T[], key: string) =>
  tiles.find((t) => t.key === key);

describe("spaceEntries", () => {
  it("sorts a finished folder largest first and drops empty entries", () => {
    const entries = spaceEntries([node(1, 5), node(2, 0), node(3, 9)], null);
    expect(entries.map((e) => e.key)).toEqual(["3", "1"]);
  });

  it("keeps finish order while scanning and groups the running part last", () => {
    const first = spaceEntries([node(1, 5), node(2, 3, false)], 95);
    expect(first.map((e) => e.key)).toEqual(["1", UNMEASURED_KEY]);
    // The running folder and the unreached rest share the remainder.
    expect(first[1]?.group?.map((e) => [e.key, e.bytes, e.live])).toEqual([
      ["2", 3, true],
      [UNMEASURED_KEY, 92, false],
    ]);
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

  it("grows running folders in the remainder without moving finished tiles", () => {
    const at = (running: number) =>
      layoutSpace(
        spaceEntries([node(1, 100), node(2, running, false)], 900, ["1"]),
        800,
        360
      );
    // Laid out beside the finished tile, the growing folder would share its
    // row and squeeze it every second.
    const early = at(20);
    const later = at(200);
    expect(tileAt(later, "1")).toEqual(tileAt(early, "1"));
    const area = (t?: { w: number; h: number }) => (t ? t.w * t.h : 0);
    expect(area(tileAt(later, "2"))).toBeGreaterThan(
      area(tileAt(early, "2")) * 3
    );
    expect(tileAt(later, "2")).toMatchObject({ live: true });
    expect(area(tileAt(later, UNMEASURED_KEY))).toBeLessThan(
      area(tileAt(early, UNMEASURED_KEY))
    );
  });

  it("frames a folder with its own entries below its name row", () => {
    const big = node(1, 80, true, [node(11, 50), node(12, 30)]);
    const small = node(2, 1, true, [node(21, 1)]);
    const tiles = layoutSpace(spaceEntries([big, small], null), 600, 300);
    const frame = tileAt(tiles, "1");
    expect(frame).toMatchObject({ frame: true, depth: 1 });
    const inner = tiles.filter((t) => t.depth === 2);
    expect(inner.map((t) => t.key)).toEqual(["1/11", "1/12"]);
    for (const t of inner) {
      expect(t.parent?.key).toBe("1");
      expect(t.y).toBeGreaterThanOrEqual((frame?.y ?? 0) + FRAME_HEADER);
      expect(t.x + t.w).toBeLessThanOrEqual((frame?.x ?? 0) + (frame?.w ?? 0));
    }
    // Too small to hold a name row and entries: a plain tile.
    expect(tileAt(tiles, "2")).toMatchObject({ frame: false });
    expect(tileAt(tiles, "2/21")).toBeUndefined();
  });

  it("leaves out entries rounding gives no area", () => {
    const tiles = layoutSpace(
      spaceEntries([node(1, 1_000_000), node(2, 1)], null),
      400,
      200
    );
    expect(tiles.map((t) => t.key)).toEqual(["1"]);
  });

  it("draws nothing for an empty folder or box", () => {
    expect(layoutSpace([], 100, 100)).toEqual([]);
    expect(layoutSpace(spaceEntries([node(1, 5)], null), 0, 100)).toEqual([]);
  });
});

describe("framePreview", () => {
  it("follows a folder holding nearly all of its parent, keeping the rest", () => {
    const riley = node(3, 90, true, [node(31, 60), node(32, 30)]);
    const users = node(1, 100, true, [riley, node(4, 10)]);
    const { chain, entries } = framePreview(users);
    expect(chain.map((c) => c.name)).toEqual(["n3"]);
    expect(entries.map((e) => e.id)).toEqual([31, 32, 4]);
    expect(framePreview(riley).chain).toEqual([]);
  });
});

describe("tileLabel", () => {
  it("shows what fits, and the size whole or not at all", () => {
    expect(tileLabel(120, 60)).toBe("full");
    expect(tileLabel(60, 24)).toBe("name");
    expect(tileLabel(20, 60)).toBe("none");
    expect(tileLabel(70, 60, "123.4 GB so far")).toBe("name");
  });
});
