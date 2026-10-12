import type {
  DockerStorageKind,
  DockerStorageResource,
} from "@core/generated/bindings";
export const storageKindLabel: Record<DockerStorageKind, string> = {
  container: "Containers",
  image: "Images",
  volume: "Volumes",
  build_cache: "Build cache",
};
export type StorageSort = "size" | "name" | "references" | "age";
export function storageRows(
  resources: readonly DockerStorageResource[],
  query: string,
  category: string,
  age: string,
  sort: StorageSort
) {
  const needle = query.trim().toLocaleLowerCase();
  return resources
    .filter((row) => {
      if (
        !`${row.name} ${row.id} ${row.project ?? ""} ${row.database ?? ""} ${row.references.join(" ")}`
          .toLocaleLowerCase()
          .includes(needle)
      )
        return false;
      if (category !== "all" && row.kind !== category) return false;
      if (
        category === "container" &&
        row.age_basis !== "stopped" &&
        !row.can_delete
      )
        return false;
      if (
        (category === "image" || category === "volume") &&
        row.references.length > 0
      )
        return false;
      const hasAgeScope =
        row.kind === "build_cache" ||
        (row.kind === "container" &&
          (row.can_delete || row.age_basis === "stopped"));
      if (hasAgeScope && age !== "all") {
        if (age === "unknown") return row.age_ms === null;
        if (row.age_ms === null || row.age_ms < Number(age) * 86_400_000)
          return false;
      }
      return true;
    })
    .sort((a, b) => {
      const result =
        sort === "name"
          ? a.name.localeCompare(b.name)
          : sort === "references"
            ? b.references.length - a.references.length
            : sort === "age"
              ? (b.age_ms ?? -1) - (a.age_ms ?? -1)
              : (b.size_bytes ?? -1) - (a.size_bytes ?? -1);
      return result || a.id.localeCompare(b.id);
    });
}
export const storageResourceKey = (
  row: Pick<DockerStorageResource, "kind" | "id">
) => `${row.kind}:${row.id}`;
export function storageAgeLabel(row: DockerStorageResource) {
  if (row.kind === "image" || row.kind === "volume")
    return "Unused age unknown";
  if (row.age_ms === null) return "Age unknown";
  const days = Math.floor(row.age_ms / 86_400_000);
  return `${row.age_basis === "stopped" ? "Stopped" : "Last used"} ${days < 1 ? "under 1 day" : `${days} days`} ago`;
}
