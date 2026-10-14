import {
  dockerPersistenceLabel,
  dockerRuntimeScopeLabel,
} from "@core/docker-labels";
import { formatBytes, formatPercent } from "@core/format";
import type { DockerSnapshot } from "@core/generated/bindings";
import { RetainedHistory } from "./retained-history";

export function DockerRuntime({ snapshot }: { snapshot: DockerSnapshot }) {
  return (
    <details className="border-border-subtle border-y py-3 text-[12px]">
      <summary className="cursor-pointer focus-visible:ring-2 focus-visible:ring-ring">
        Runtime &amp; history
      </summary>
      <div className="mt-3 space-y-3">
        <div>
          <p>
            Runtime processes on this Mac{" "}
            <span className="figures ml-3">
              CPU {formatPercent(snapshot.runtime.cpu_pct)} · Footprint{" "}
              {formatBytes(snapshot.runtime.footprint_bytes)}
            </span>{" "}
            ·{" "}
            {dockerRuntimeScopeLabel[snapshot.runtime.scope] ??
              snapshot.runtime.scope}
          </p>
          <p className="mt-2 text-muted-foreground">
            {snapshot.runtime.explanation}
            {snapshot.engine?.capacity_observed_ms != null
              ? ` Engine capacity observed ${new Date(snapshot.engine.capacity_observed_ms).toLocaleString()}.`
              : " Engine capacity measurement time unavailable."}
          </p>
          <p className="mt-1 text-muted-foreground">
            {snapshot.runtime.processes.join(" · ") ||
              "No verified runtime processes"}
            {snapshot.runtime.observed_ms !== null
              ? ` · observed ${new Date(snapshot.runtime.observed_ms).toLocaleTimeString()}`
              : ""}
          </p>
        </div>
        <p className="text-[11px] text-muted-foreground">
          History:{" "}
          {dockerPersistenceLabel[snapshot.persistence.status] ??
            snapshot.persistence.status}{" "}
          · {formatBytes(snapshot.persistence.logical_bytes)} /{" "}
          {formatBytes(snapshot.persistence.allowance_bytes)} logical allowance
          {snapshot.persistence.retained_from_ms !== null
            ? ` · retained from ${new Date(snapshot.persistence.retained_from_ms).toLocaleString()}`
            : ""}
        </p>
        <RetainedHistory enabled generation={snapshot.generation} />
      </div>
    </details>
  );
}
