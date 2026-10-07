import { exportFailure, exportFileName, exportSaved } from "./export";

describe("export helpers", () => {
  it("names the file after the range's local start and its length", () => {
    const from = new Date(2026, 9, 4, 9, 5).getTime();
    expect(exportFileName("24h", from)).toBe("kelvo-2026-10-04-0905-24h.csv");
    expect(exportFileName("6h", from)).toBe("kelvo-2026-10-04-0905-6h.csv");
  });

  it("says where a save went and is silent on a cancel", () => {
    expect(exportSaved({ kind: "cancelled" })).toBeNull();
    expect(
      exportSaved({
        kind: "saved",
        path: "/Users/me/kelvo.csv",
        rows: 1440,
        gap_rows: 2,
        bytes: 1,
      })
    ).toBe("Exported 1,440 rows and 2 gaps to /Users/me/kelvo.csv");
    expect(
      exportSaved({
        kind: "saved",
        path: "/x.csv",
        rows: 1,
        gap_rows: 0,
        bytes: 1,
      })
    ).toBe("Exported 1 row to /x.csv");
  });

  it("explains failures", () => {
    expect(exportFailure({ kind: "export", message: "disk full" })).toBe(
      "Export failed: disk full"
    );
    expect(exportFailure({ kind: "history_unavailable" })).toBe(
      "Export failed: no history is being kept."
    );
    expect(exportFailure({ kind: "store_busy", message: "x" })).toMatch(/busy/);
    expect(exportFailure({ kind: "internal", message: "boom" })).toBe(
      "Export failed (internal): boom"
    );
  });
});
