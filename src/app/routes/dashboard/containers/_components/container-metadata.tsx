import type { DockerContainer } from "@core/generated/bindings";
import { Button } from "~/components/ui/button";
import { lifecycleLabel } from "../_lib/presentation";
export function ContainerMetadata({
  row,
  refreshing,
  onRefresh,
}: {
  row: DockerContainer;
  refreshing: boolean;
  onRefresh: () => void;
}) {
  return (
    <details className="border-border-subtle border-y py-3 text-[12px]">
      <summary className="cursor-pointer">Container metadata</summary>
      <Button
        size="sm"
        variant="outline"
        className="mt-2"
        disabled={refreshing}
        onClick={onRefresh}
      >
        {refreshing ? "Refreshing metadata…" : "Refresh metadata"}
      </Button>
      {!row.metadata_available && (
        <p className="mt-2 text-muted-foreground">
          Inspection metadata unavailable. Inventory facts are shown below; an
          unknown limit or quota does not mean unrestricted.
        </p>
      )}
      <dl className="mt-3 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-6 gap-y-2">
        {[
          ["ID", row.id],
          ["Engine", row.engine_id],
          ["State", lifecycleLabel[row.lifecycle]],
          ["Created", row.created ?? "Unknown"],
          [
            "CPU quota",
            !row.metadata_available
              ? "Unavailable"
              : row.cpu_quota === null
                ? "No explicit quota"
                : String(row.cpu_quota),
          ],
          ["Restart policy", row.restart_policy ?? "Unknown"],
          [
            "Ports",
            !row.metadata_available
              ? "Unavailable"
              : row.ports.join(" · ") || "None",
          ],
          [
            "Mounts",
            !row.metadata_available
              ? "Unavailable"
              : row.mounts.join(" · ") || "None",
          ],
        ].map(([label, value]) => (
          <div key={label} className="contents">
            <dt className="text-muted-foreground">{label}</dt>
            <dd className="break-all">{value}</dd>
          </div>
        ))}
      </dl>
    </details>
  );
}
