import { mockContainer } from "@core/mock/docker";
import { describe, expect, it } from "vitest";
import {
  filterContainers,
  sortedContainers,
  withInspection,
} from "./presentation";

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

it("accepts fresh inspection metadata for old records without a metadata timestamp", () => {
  const legacy = mockContainer(0, 1000);
  delete legacy.metadata_observed_ms;
  const inspected = {
    ...mockContainer(0, 2000),
    explicit_limit_bytes: 4_000_000,
  };
  expect(withInspection(legacy, inspected)?.explicit_limit_bytes).toBe(
    4_000_000
  );
  const missingStamp = { ...inspected };
  delete missingStamp.metadata_observed_ms;
  expect(withInspection(legacy, missingStamp)?.explicit_limit_bytes).toBe(
    legacy.explicit_limit_bytes
  );
});
