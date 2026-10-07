import { describe, expect, it } from "vitest";
import { diskSubtitle, volumeRows } from "./disk";

describe("disk page helpers", () => {
  it("puts the boot container's mounts first, in Rust's order", () => {
    const rows = volumeRows(
      ["/Volumes/Backup", "/System/Volumes/Data", "/"],
      {
        "disk.used{vol=/}": 58e9,
        "disk.total{vol=/}": 1e12,
      },
      ["/", "/System/Volumes/Data"]
    );
    expect(rows.map((r) => r.id)).toEqual([
      "/",
      "/System/Volumes/Data",
      "/Volumes/Backup",
    ]);
    expect(rows[0]).toMatchObject({
      usedBytes: 58e9,
      totalBytes: 1e12,
      freeBytes: null,
    });
  });

  it("sorts by mount point without boot mounts", () => {
    const rows = volumeRows(["/Volumes/B", "/", "/Volumes/A"], {}, []);
    expect(rows.map((r) => r.id)).toEqual(["/", "/Volumes/A", "/Volumes/B"]);
  });

  it("writes the subtitle from devices and volumes", () => {
    expect(diskSubtitle(["disk3"], 2)).toBe("disk3 · 2 volumes");
    expect(diskSubtitle([], 1)).toBe("1 volume");
  });
});
