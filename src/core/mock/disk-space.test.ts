import type { SpaceSubtree } from "@core/generated/bindings";
import type { CommandResult } from "@core/transport";
import { describe, expect, it } from "vitest";
import { createMockDiskSpace, MOCK_SCAN_MS } from "./disk-space";
import { BOOT_UNATTRIBUTED } from "./disk-space-fixture";
import type { ScenarioName } from "./fixtures";

function setup(scenarios: ScenarioName[] = []) {
  let t = 1_000_000;
  const space = createMockDiskSpace(scenarios, () => t);
  return {
    space,
    advance: (ms: number) => {
      t += ms;
    },
  };
}

function data<T>(r: CommandResult<T>): T {
  if (r.status !== "ok") throw new Error(JSON.stringify(r.error));
  return r.data;
}

const sumDone = (tree: SpaceSubtree) =>
  tree.children.filter((c) => c.done).reduce((a, c) => a + (c.bytes ?? 0), 0);

describe("mock disk space", () => {
  it("fills the root in progressively against used bytes", async () => {
    const { space, advance } = setup();
    const started = data(await space.diskSpaceStart("/", []));
    const id = started.report_id ?? -1;
    const first = data(await space.diskSpaceSubtree(id, null));
    expect(first.folder.done).toBe(false);
    expect(first.children.map((c) => c.name)).toContain("Users");
    expect(first.unmeasured_bytes).toBeGreaterThan(0);
    advance(MOCK_SCAN_MS / 2);
    const mid = data(await space.diskSpaceSubtree(id, null));
    expect((mid.unmeasured_bytes ?? 0) + sumDone(mid)).toBeCloseTo(
      mid.total_bytes ?? 0
    );
    advance(MOCK_SCAN_MS);
    const done = data(await space.diskSpaceSubtree(id, null));
    expect(done.folder.done).toBe(true);
    expect(done.unmeasured_bytes).toBeNull();
    expect(sumDone(done)).toBeCloseTo(done.total_bytes ?? 0, -3);
  });

  it("puts skipped protected folders into Other and lists them", async () => {
    const { space, advance } = setup();
    const id = data(await space.diskSpaceStart("/", [])).report_id ?? -1;
    advance(MOCK_SCAN_MS);
    const report = await space.diskSpaceReport();
    expect(report?.id).toBe(id);
    expect(report?.skipped).toContain("Downloads");
    expect(report?.other_bytes).toBeGreaterThan(BOOT_UNATTRIBUTED + 22e9);
    const included = setup();
    const keys = data(await included.space.diskSpaceVolumes())[0]?.protected;
    await included.space.diskSpaceStart(
      "/",
      (keys ?? []).map((p) => p.key)
    );
    included.advance(MOCK_SCAN_MS);
    const full = await included.space.diskSpaceReport();
    expect(full?.skipped).toEqual([]);
    expect(full?.other_bytes).toBeCloseTo(BOOT_UNATTRIBUTED, -3);
  });

  it("drills into finished folders and folds small entries", async () => {
    const { space, advance } = setup();
    const id = data(await space.diskSpaceStart("/", [])).report_id ?? -1;
    advance(MOCK_SCAN_MS);
    const root = data(await space.diskSpaceSubtree(id, null));
    const users = root.children.find((c) => c.name === "Users");
    const sub = data(await space.diskSpaceSubtree(id, users?.id ?? null));
    expect(sub.path.map((c) => c.name)).toEqual(["Macintosh HD", "Users"]);
    expect(sub.children.some((c) => c.kind === "smaller")).toBe(true);
    const files = data(await space.diskSpaceLargestFiles(id));
    expect(files).toHaveLength(50);
    expect(files[0]?.bytes).toBeGreaterThanOrEqual(files[1]?.bytes ?? 0);
  });

  it("runs one scan at a time and keeps the last report on cancel", async () => {
    const { space, advance } = setup();
    await space.diskSpaceStart("/", []);
    advance(MOCK_SCAN_MS);
    const first = (await space.diskSpaceReport())?.id;
    await space.diskSpaceStart("/", []);
    expect((await space.diskSpaceStart("/", [])).status).toBe("error");
    await space.diskSpaceCancel();
    expect((await space.diskSpaceStatus()).phase).toBe("cancelled");
    expect((await space.diskSpaceReport())?.id).toBe(first);
  });

  it("fails a scan whose volume disconnects", async () => {
    const { space, advance } = setup(["disk-space-eject"]);
    await space.diskSpaceStart("/Volumes/SanDisk Extreme", []);
    advance(MOCK_SCAN_MS);
    const status = await space.diskSpaceStatus();
    expect(status.phase).toBe("failed");
    expect(status.error).toContain("disconnected");
    expect(await space.diskSpaceReport()).toBeNull();
  });

  it("is unavailable in the App Store edition", async () => {
    const { space } = setup(["appstore"]);
    expect((await space.diskSpaceVolumes()).status).toBe("error");
    expect((await space.diskSpaceStart("/", [])).status).toBe("error");
  });
});
