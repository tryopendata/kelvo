import { formatBytes } from "@core/format";
import { useEffect, useState } from "react";
import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "~/components/ui/dialog";
import type { useDockerCleanup } from "../_hooks/use-docker-cleanup";

export type CleanupFlow = ReturnType<typeof useDockerCleanup>;
const statusLabels = {
  succeeded: "Succeeded",
  skipped: "Skipped",
  failed: "Failed",
  outcome_unknown: "Outcome unknown",
};

export function DockerCleanupFlow({ flow }: { flow: CleanupFlow }) {
  const [volumeStep, setVolumeStep] = useState(false);
  const [acknowledged, setAcknowledged] = useState(false);
  const [expired, setExpired] = useState(false);
  const review = flow.review;
  useEffect(() => {
    setVolumeStep(false);
    setAcknowledged(false);
    setExpired(false);
    if (!review) return;
    const remaining = review.expires_ms - Date.now();
    if (remaining <= 0) {
      setExpired(true);
      return;
    }
    const timer = setTimeout(() => setExpired(true), remaining);
    return () => clearTimeout(timer);
  }, [review]);
  const volumes =
    review?.resources.filter((row) => row.kind === "volume") ?? [];
  const stops =
    review?.selections.some((item) => item.action === "stop") ?? false;
  return (
    <>
      {(flow.error || flow.operationError) && (
        <p role="alert" className="text-[12px] text-destructive">
          {flow.error ?? String(flow.operationError)}
        </p>
      )}
      {flow.operation && (
        <section
          aria-label="Docker operation results"
          className="border-border-subtle border-y py-3 text-[12px]"
        >
          <h2 className="font-[590]">
            {flow.operation.finished_ms === null
              ? "Working…"
              : "Operation complete"}
          </h2>
          <p className="mt-1 text-muted-foreground">
            {flow.operation.results.length} / {flow.operation.total} processed.
            Operations continue when you leave this page.
          </p>
          <ul className="mt-2 space-y-1">
            {flow.operation.results.map((item) => (
              <li
                key={`${item.selection.kind}:${item.selection.id}:${item.selection.action}`}
              >
                <span className="font-[590]">{statusLabels[item.status]}</span>{" "}
                · {item.selection.id} · {item.message}
                {item.reclaimed_bytes !== null && (
                  <span className="figures">
                    {" "}
                    · {formatBytes(item.reclaimed_bytes)} reported reclaimed by
                    Docker
                  </span>
                )}
              </li>
            ))}
          </ul>
          <p className="mt-2 text-muted-foreground">
            Unknown outcomes are not retried. Refreshed storage is an
            observation, not a promise of immediate Mac free space.
          </p>
        </section>
      )}
      <Dialog
        open={Boolean(review)}
        onOpenChange={(open) => {
          if (!open) flow.dismiss();
        }}
      >
        <DialogContent>
          <DialogHeader>
            <DialogTitle>
              {volumeStep
                ? "Permanently delete volumes?"
                : stops
                  ? "Review Stop"
                  : "Review cleanup"}
            </DialogTitle>
            <DialogDescription>
              {volumeStep
                ? "Confirm the entire cleanup below. All data in the named volumes will be permanently lost; the other selected resources will also be removed."
                : stops
                  ? "Docker waits 30 seconds for a graceful stop, then terminates the container if necessary."
                  : "Only the selected resources below will be removed. Container removal keeps its volumes; bind-mounted host files are never deleted."}
            </DialogDescription>
          </DialogHeader>
          <ul className="max-h-48 min-w-0 space-y-2 overflow-y-auto break-words text-[12px]">
            {(review?.resources ?? []).map((row) => (
              <li key={`${row.kind}:${row.id}`}>
                <span className="font-[590]">{row.name}</span> ·{" "}
                {row.kind.replaceAll("_", " ")} ·{" "}
                <span className="figures">{formatBytes(row.size_bytes)}</span>
                <div className="text-muted-foreground">
                  {row.references.length
                    ? `References: ${row.references.join(", ")}`
                    : "No container references"}
                </div>
              </li>
            ))}
          </ul>
          {!volumeStep && !stops && (
            <p className="text-[12px] text-muted-foreground">
              Estimated reclaimable:{" "}
              {review?.estimated_reclaimed_bytes == null
                ? review?.resources.some(
                    (row) => row.kind === "image" || row.kind === "build_cache"
                  )
                  ? "Unknown; shared image and cache layers cannot be added safely."
                  : "Estimate unavailable."
                : formatBytes(review.estimated_reclaimed_bytes)}
            </p>
          )}
          {review?.warnings.map((warning) => (
            <p key={warning} className="text-[12px] text-muted-foreground">
              {warning}
            </p>
          ))}
          {volumeStep && (
            <>
              <p className="text-[12px] text-muted-foreground">
                Docker volume names can be reused. If another Docker client
                replaces an unused volume between the final check and deletion,
                Docker cannot reliably detect that replacement.
              </p>
              <label
                className="flex items-start gap-2 text-[12px]"
                htmlFor="volume-data-loss"
              >
                <Checkbox
                  id="volume-data-loss"
                  checked={acknowledged}
                  onCheckedChange={(value) => setAcknowledged(value === true)}
                />
                I understand the permanent data loss and volume name reuse risk
                for the listed volumes.
              </label>
            </>
          )}
          {expired && (
            <p role="alert" className="text-[12px] text-destructive">
              This review expired. Cancel and review the selection again.
            </p>
          )}
          <DialogFooter>
            <Button variant="outline" onClick={flow.dismiss} autoFocus>
              Cancel
            </Button>
            <Button
              variant="destructive"
              disabled={flow.busy || expired || (volumeStep && !acknowledged)}
              onClick={() => {
                if (!volumeStep && volumes.length) setVolumeStep(true);
                else void flow.execute(volumeStep && acknowledged);
              }}
            >
              {volumeStep
                ? "Delete selected resources"
                : volumes.length
                  ? "Review volume deletion"
                  : stops
                    ? "Stop containers"
                    : "Delete selected resources"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
