import type {
  DockerCoverage,
  DockerPersistence,
  DockerSnapshot,
} from "@core/generated/bindings";
import { useState } from "react";
import { Button } from "~/components/ui/button";
import {
  eventCoverage,
  HistoryDetails,
  mergeCoverage,
} from "./history-details";

// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Controller and presentation split tracked in architecture refactor package H.
export function HistoryNotice({
  snapshot,
  persistence = snapshot.persistence,
  coverage,
  metric,
  onManage,
  onResume,
}: {
  snapshot: DockerSnapshot;
  persistence?: DockerPersistence | undefined;
  coverage: readonly DockerCoverage[];
  metric: "cpu" | "memory";
  onManage: () => void;
  onResume: () => void;
}) {
  const [open, setOpen] = useState(false);
  const records = mergeCoverage(coverage);
  const unavailable = [
    "unavailable",
    "permission_denied",
    "incompatible",
  ].includes(snapshot.status);
  const paused = ["paused", "idle_suspended"].includes(snapshot.status);
  const saving = persistence.status === "write_failed";
  const gaps = records.filter((record) => !eventCoverage(record));
  const partial =
    snapshot.aggregate.partial ||
    snapshot.aggregate.expected < snapshot.aggregate.running;
  const events = records.some(eventCoverage);
  const matches = [
    unavailable,
    paused,
    saving,
    gaps.length > 0,
    partial,
    events,
  ].filter(Boolean).length;
  if (!matches) return null;
  const observations = snapshot.containers.flatMap((row) =>
    row.observed_ms === null ? [] : [row.observed_ms]
  );
  const latest = observations.length
    ? Math.max(...observations)
    : snapshot.observed_ms;
  const runtime = snapshot.engine?.runtime ?? "Engine";
  const text = unavailable
    ? saving
      ? `${runtime} unavailable; recent history not saved`
      : `${runtime} unavailable · last reading ${new Date(latest).toLocaleTimeString()}`
    : paused
      ? "Monitoring paused"
      : saving
        ? "Recent history is not being saved. Kelvo will retry."
        : gaps.length
          ? `${metric === "cpu" ? "CPU" : "Memory"} history missing for ${gaps.length} ${gaps.length === 1 ? "interval" : "intervals"}`
          : partial
            ? `Monitoring ${snapshot.aggregate.expected} of ${snapshot.aggregate.running} running containers`
            : "Some container events may be missing";
  const details = [...records];
  if (unavailable || paused)
    details.push({
      start_ms: latest,
      end_ms: null,
      container_id: null,
      reason: snapshot.status,
    });
  if (saving && !records.some((r) => r.reason === "write_failed"))
    details.push({
      start_ms: latest,
      end_ms: null,
      container_id: null,
      reason: "write_failed",
    });
  if (partial)
    details.push({
      start_ms: latest,
      end_ms: null,
      container_id: null,
      reason: "partial_population",
    });
  return (
    <>
      <div
        role="status"
        className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground"
      >
        <span className="flex-1">{text}</span>
        {(unavailable || paused || partial) && (
          <Button
            size="sm"
            variant="ghost"
            onClick={unavailable || paused ? onResume : onManage}
          >
            {unavailable ? "Retry" : paused ? "Resume" : "Monitoring…"}
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={() => setOpen(true)}>
          Details ({mergeCoverage(details).length})
        </Button>
      </div>
      <HistoryDetails
        open={open}
        onOpenChange={setOpen}
        savingError={saving ? persistence.error : null}
        records={details}
        containers={snapshot.containers}
      />
    </>
  );
}
