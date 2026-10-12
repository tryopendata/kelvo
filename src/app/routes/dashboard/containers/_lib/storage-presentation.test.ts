import { mockDockerSnapshot } from "@core/mock/docker";
import { mockStorageSnapshot } from "@core/mock/docker-storage";
import { expect, it } from "vitest";
import { storageAgeLabel, storageRows } from "./storage-presentation";

it("filters actual stopped/last-used evidence without inventing unused age for images/volumes", () => {
  const rows = mockStorageSnapshot(mockDockerSnapshot(32, 0), 0).resources;
  expect(
    storageRows(rows, "", "container", "7", "size").length
  ).toBeGreaterThan(0);
  expect(storageRows(rows, "", "container", "30", "size")).toHaveLength(0);
  const volumes = storageRows(rows, "", "volume", "30", "size");
  expect(volumes.map((row) => row.name)).toEqual(["old-database-data"]);
  const volume = volumes[0];
  if (!volume) throw new Error("Missing unreferenced volume fixture");
  expect(storageAgeLabel(volume)).toBe("Unused age unknown");
  expect(storageRows(rows, "", "build_cache", "30", "size")).toHaveLength(1);
});

it("keeps unknown ages accessible, searches database inference and sorts null sizes last", () => {
  const rows = mockStorageSnapshot(mockDockerSnapshot(32, 0), 0).resources;
  const stopped = rows.find(
    (row) => row.kind === "container" && row.can_delete
  );
  if (!stopped) throw new Error("Missing stopped fixture");
  const unknown = { ...stopped, id: "unknown", age_ms: null, size_bytes: null };
  expect(storageRows([unknown], "", "container", "7", "size")).toHaveLength(0);
  expect(
    storageRows([unknown], "", "container", "unknown", "size")
  ).toHaveLength(1);
  expect(
    storageRows([unknown, stopped], "", "all", "all", "size").at(-1)?.id
  ).toBe("unknown");
  expect(
    storageRows(rows, "PostgreSQL", "all", "all", "name").length
  ).toBeGreaterThan(0);
});
