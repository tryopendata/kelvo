import { mockContainer } from "@core/mock/docker";
import { describe, expect, it } from "vitest";
import { filterContainers, sortedContainers } from "./presentation";

describe("container view shaping", () => {
  it("searches official project, image and name without dropping exited rows", () => {
    const rows = Array.from({ length: 8 }, (_, i) => mockContainer(i, 1000));
    expect(filterContainers(rows, "PROJECT-2", "all").map((r) => r.id)).toEqual(
      ["container-5", "container-6", "container-7"]
    );
    expect(filterContainers(rows, "nginx", "exited").map((r) => r.id)).toEqual([
      "container-6",
    ]);
    expect(rows).toHaveLength(8);
  });
  it("uses stable container identity to break ties, placing unknown metrics last", () => {
    const a = mockContainer(1, 1000);
    const b = { ...mockContainer(2, 1000), metrics: { ...a.metrics } };
    const missing = {
      ...mockContainer(3, 1000),
      metrics: { ...a.metrics, cpu_pct: null },
    };
    expect(sortedContainers([missing, b, a], "cpu").map((r) => r.id)).toEqual([
      a.id,
      b.id,
      missing.id,
    ]);
  });
});
