import { PROCESSES, withPorts } from "@core/mock/fixtures";
import { describe, expect, it } from "vitest";
import {
  COLUMN_SETS,
  columnSetOptions,
  countLine,
  filterProcesses,
  hiddenNote,
} from "./processes";

describe("columnSetOptions", () => {
  it("offers Network only with per-process network (D-081)", () => {
    expect(columnSetOptions(true, false).map((o) => o.value)).toEqual([
      "cpu",
      "memory",
      "energy",
      "disk",
      "network",
    ]);
    expect(columnSetOptions(false, true).map((o) => o.value)).not.toContain(
      "network"
    );
    expect(COLUMN_SETS.network.sort).toEqual({ by: "netTotal", dir: "desc" });
  });

  it("offers GPU only with per-process GPU time (D-085)", () => {
    expect(columnSetOptions(true, true).map((o) => o.value)).toEqual([
      "cpu",
      "memory",
      "energy",
      "disk",
      "network",
      "gpu",
    ]);
    expect(columnSetOptions(true, false).map((o) => o.value)).not.toContain(
      "gpu"
    );
    expect(COLUMN_SETS.gpu.sort).toEqual({ by: "gpu", dir: "desc" });
  });
});

describe("filterProcesses", () => {
  it("matches names case-insensitively and PIDs by prefix", () => {
    expect(filterProcesses(PROCESSES, "XCO").map((p) => p.name)).toEqual([
      "Xcode",
    ]);
    expect(filterProcesses(PROCESSES, "18").map((p) => p.pid)).toEqual([1840]);
    expect(filterProcesses(PROCESSES, "  ")).toBe(PROCESSES);
  });

  it("matches listening ports by prefix", () => {
    const rows = withPorts(PROCESSES);
    expect(filterProcesses(rows, "5432").map((p) => p.name)).toEqual([
      "com.docker.backend",
    ]);
    expect(filterProcesses(rows, "51").map((p) => p.pid)).toEqual([5531]);
    expect(filterProcesses(rows, "300").map((p) => p.pid)).toEqual([5531]);
  });

  it("does not treat digits inside a name query as a PID", () => {
    expect(filterProcesses(PROCESSES, "node1")).toEqual([]);
  });
});

describe("countLine", () => {
  it("counts all processes and the threads of the shown ones", () => {
    const shown = PROCESSES.slice(0, 2);
    expect(countLine(PROCESSES, PROCESSES)).toMatch(
      /^8 processes · [\d,]+ threads$/
    );
    expect(countLine(PROCESSES, shown)).toBe("2 of 8 processes · 686 threads");
  });
});

describe("hiddenNote", () => {
  it("states the count when known and says it plainly when not", () => {
    expect(hiddenNote(335)).toMatch(/^335 processes hidden/);
    expect(hiddenNote(0)).toBe("");
    expect(hiddenNote(null)).toMatch(/hidden/);
  });
});
