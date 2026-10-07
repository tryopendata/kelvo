import { PROCESSES, withGpuPct } from "@core/mock/fixtures";
import { describe, expect, it } from "vitest";
import { measuredGpu } from "./gpu";

describe("measuredGpu", () => {
  it("keeps rows with a measured, non-zero share", () => {
    expect(measuredGpu(PROCESSES)).toEqual([]);
    expect(measuredGpu(withGpuPct(PROCESSES)).map((p) => p.name)).toEqual([
      "Xcode",
      "WindowServer",
      "Safari",
      "Figma",
      "com.docker.backend",
    ]);
  });
});
