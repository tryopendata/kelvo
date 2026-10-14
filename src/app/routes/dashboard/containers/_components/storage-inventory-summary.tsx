import { formatBytes } from "@core/format";
import type { DockerStorageSnapshot } from "@core/generated/bindings";
import { storageKindLabel } from "../_lib/storage-presentation";
export function StorageInventorySummary({
  snapshot,
}: {
  snapshot: DockerStorageSnapshot;
}) {
  return (
    <>
      {" "}
      <dl className="flex flex-wrap gap-x-8 gap-y-3 border-border-subtle border-y py-3">
        {snapshot.categories.map((item) => (
          <div key={item.kind}>
            <dt className="text-[11px] text-muted-foreground">
              {storageKindLabel[item.kind]}
            </dt>
            <dd className="figures-display mt-1 text-[20px]">
              {formatBytes(item.size_bytes)}
            </dd>
            <dd className="figures text-[11px] text-muted-foreground">
              {item.reclaimable_bytes === null
                ? "Reclaimable unknown"
                : `${formatBytes(item.reclaimable_bytes)} potentially reclaimable`}
            </dd>
          </div>
        ))}
      </dl>
      <p className="text-[11px] text-muted-foreground">
        Shared image and cache layers make sizes non-additive. VM allocation,
        logs, bind-mounted host files and other Buildx builders may use
        additional space. Unreferenced does not mean disposable.
      </p>
      {!snapshot.complete && (
        <p role="status" className="text-[12px] text-muted-foreground">
          Incomplete inventory. Cleanup is unavailable until references can be
          verified.
        </p>
      )}
      {snapshot.warnings.map((warning) => (
        <p key={warning} className="text-[12px] text-muted-foreground">
          {warning}
        </p>
      ))}
    </>
  );
}
