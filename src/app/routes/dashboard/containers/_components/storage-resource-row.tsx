import { formatBytes } from "@core/format";
import type { DockerStorageResource } from "@core/generated/bindings";
import { Checkbox } from "~/components/ui/checkbox";
import {
  storageAgeLabel,
  storageKindLabel,
  storageResourceKey,
} from "../_lib/storage-presentation";

export function StorageResourceRow({
  row,
  selected,
  disabled,
  selectionFull,
  onToggle,
}: {
  row: DockerStorageResource;
  selected: boolean;
  disabled: boolean;
  selectionFull: boolean;
  onToggle: (checked: boolean | "indeterminate") => void;
}) {
  return (
    <tr className="border-border-subtle border-t align-top hover:bg-selected/50">
      <td className="max-w-72 px-3 py-2">
        <label
          htmlFor={`storage-${storageResourceKey(row)}`}
          className="flex items-start gap-2"
        >
          <Checkbox
            id={`storage-${storageResourceKey(row)}`}
            aria-label={`Select ${row.name}`}
            checked={selected && row.can_delete}
            disabled={
              disabled || !row.can_delete || (!selected && selectionFull)
            }
            onCheckedChange={onToggle}
          />
          <span className="min-w-0 break-words">
            {row.name}
            <span className="block text-[11px] text-muted-foreground">
              {storageKindLabel[row.kind]} ·{" "}
              {row.project ?? "No Compose project"}
            </span>
          </span>
        </label>
        {row.database && (
          <p className="mt-1 text-[11px] text-muted-foreground">
            Likely {row.database} database · inferred from image
          </p>
        )}
        {row.kind === "volume" && !row.references.length && !row.database && (
          <p className="mt-1 text-[11px] text-muted-foreground">
            Previous use unknown
          </p>
        )}
        {row.mounts.length > 0 && (
          <details className="mt-1 text-[11px] text-muted-foreground">
            <summary className="cursor-pointer">
              {row.mounts.length} mounts
            </summary>
            <ul>
              {row.mounts.map((mount) => (
                <li key={`${mount.kind}:${mount.source}:${mount.destination}`}>
                  {mount.kind} ·{" "}
                  {mount.name ?? mount.source ?? "Unknown source"} →{" "}
                  {mount.destination ?? "Unknown destination"}
                </li>
              ))}
            </ul>
          </details>
        )}
      </td>
      <td className="figures whitespace-nowrap px-3 py-2">
        {formatBytes(row.size_bytes)}
      </td>
      <td className="max-w-48 break-words px-3 py-2 text-muted-foreground">
        {row.references.length > 2 ? (
          <details>
            <summary className="cursor-pointer rounded-control focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
              {row.references.length} container references
            </summary>
            <ul
              aria-label="Container references"
              className="mt-1 max-h-40 overflow-y-auto"
            >
              {row.references.map((reference) => (
                <li key={reference}>{reference}</li>
              ))}
            </ul>
          </details>
        ) : (
          row.references.join(", ") || "No container references"
        )}
      </td>
      <td className="px-3 py-2 text-muted-foreground">
        {storageAgeLabel(row)}
      </td>
      <td className="max-w-60 px-3 py-2 text-muted-foreground">
        {row.can_delete
          ? "Available for review"
          : (row.reason ?? "Not eligible")}
      </td>
    </tr>
  );
}
