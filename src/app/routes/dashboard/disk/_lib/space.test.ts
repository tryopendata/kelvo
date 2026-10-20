import type {
  SpaceNode,
  SpaceReport,
  SpaceScanStatus,
} from "@core/generated/bindings";
import { describe, expect, it } from "vitest";
import {
  agoLabel,
  canOpen,
  elapsedLabel,
  itemsLabel,
  middleTruncate,
  shareLabel,
  spaceScreen,
} from "./space";

const status = (patch: Partial<SpaceScanStatus>): SpaceScanStatus => ({
  phase: "scanning",
  report_id: 2,
  volume: "/",
  elapsed_ms: 0,
  items: 0,
  measured_bytes: 0,
  used_bytes: 100,
  current: null,
  error: null,
  ...patch,
});

const report = (id: number, mount = "/"): SpaceReport => ({
  id,
  volume: {
    mount,
    name: "Macintosh HD",
    is_boot: true,
    used_bytes: 100,
    total_bytes: 200,
    protected: [],
  },
  finished_ms: 0,
  duration_ms: 0,
  items: 0,
  used_bytes: 100,
  files_bytes: 90,
  volumes_bytes: 0,
  other_bytes: 10,
  unreadable: 0,
  unreadable_paths: [],
  skipped: [],
  truncated: false,
});

describe("spaceScreen", () => {
  it("shows the running scan of the volume in the URL", () => {
    const { screen } = spaceScreen(status({}), report(1), "/");
    expect(screen).toMatchObject({ mode: "scan", reportId: 2 });
  });

  it("keeps a scan of another volume out of this one", () => {
    const { screen } = spaceScreen(
      status({ volume: "/Volumes/X" }),
      report(1),
      "/"
    );
    expect(screen).toMatchObject({ mode: "report", reportId: 1 });
    expect(spaceScreen(status({}), null, null).screen.mode).toBe("volumes");
  });

  it("stays on the report while a finished scan's report loads", () => {
    const done = status({ phase: "done" });
    expect(spaceScreen(done, report(1), "/").screen).toEqual({
      mode: "report",
      reportId: 2,
      report: null,
    });
    expect(spaceScreen(done, report(2), "/").screen).toMatchObject({
      mode: "report",
      reportId: 2,
      report: { id: 2 },
    });
  });

  it("explains a cancelled or failed scan and falls back to the kept report", () => {
    const cancelled = spaceScreen(
      status({ phase: "cancelled", report_id: null }),
      report(1),
      "/"
    );
    expect(cancelled.ended).toMatch(/cancelled/);
    expect(cancelled.screen.mode).toBe("report");
    const failed = spaceScreen(
      status({ phase: "failed", report_id: null, error: "Ejected." }),
      null,
      "/"
    );
    expect(failed).toEqual({ screen: { mode: "volumes" }, ended: "Ejected." });
  });
});

describe("labels", () => {
  it("formats shares, items, ages and elapsed time", () => {
    expect(shareLabel(0.1234)).toBe("12.3%");
    expect(shareLabel(0.0004)).toBe("<0.1%");
    expect(shareLabel(0)).toBe("0.0%");
    expect(itemsLabel(1)).toBe("1 item");
    expect(itemsLabel(48_200)).toBe("48,200 items");
    expect(itemsLabel(482_000)).toBe("482 K items");
    expect(itemsLabel(1_840_000)).toBe("1.8 M items");
    expect(agoLabel(10 * 60_000, 6 * 60_000)).toBe("4 min ago");
    expect(agoLabel(3 * 3_600_000, 0)).toBe("3 h ago");
    expect(agoLabel(30_000, 0)).toBe("just now");
    expect(elapsedLabel(123_400)).toBe("2:03");
  });

  it("keeps both ends of a long path", () => {
    const out = middleTruncate("Users/riley/Library/Developer/DerivedData", 20);
    expect(out).toHaveLength(20);
    expect(out.startsWith("Users/ril")).toBe(true);
    expect(out.endsWith("rivedData")).toBe(true);
    expect(middleTruncate("short", 20)).toBe("short");
  });
});

describe("canOpen", () => {
  const folder: SpaceNode = {
    id: 3,
    name: "Users",
    kind: "folder",
    bytes: 10,
    items: 1,
    done: true,
    openable: true,
    children: [],
  };
  it("opens finished folders only", () => {
    expect(canOpen(folder)).toBe(true);
    expect(canOpen({ ...folder, done: false })).toBe(false);
    expect(canOpen({ ...folder, kind: "file" })).toBe(false);
    expect(canOpen({ ...folder, id: null, kind: "smaller" })).toBe(false);
    expect(canOpen(null)).toBe(false);
  });
});
