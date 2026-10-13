import type { DockerContainer, DockerCoverage } from "@core/generated/bindings";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "~/components/ui/dialog";

export const eventCoverage = (gap: DockerCoverage) =>
  gap.reason === "events_disconnected" ||
  gap.reason === "event_replay_uncertain";
export const historyReason: Record<string, string> = {
  write_failed: "Measurements could not be saved",
  write_rejected: "History storage was full or the disk was low",
  writer_disconnected: "History writer stopped",
  events_disconnected: "Lost connection to Docker events",
  event_replay_uncertain: "Some events may be missing after reconnecting",
  app_not_running: "Kelvo was not running",
  engine_switch: "Monitoring switched Docker engines",
};
export function mergeCoverage(
  records: readonly DockerCoverage[]
): DockerCoverage[] {
  const rows: DockerCoverage[] = [];
  for (const record of [...records].sort((a, b) => a.start_ms - b.start_ms)) {
    const previous = [...rows]
      .reverse()
      .find(
        (r) =>
          r.reason === record.reason &&
          r.container_id === record.container_id &&
          (r.end_ms ?? Infinity) >= record.start_ms
      );
    if (previous)
      previous.end_ms =
        previous.end_ms === null || record.end_ms === null
          ? null
          : Math.max(previous.end_ms, record.end_ms);
    else rows.push({ ...record });
  }
  return rows;
}
const group = (row: DockerCoverage) =>
  eventCoverage(row)
    ? "Events"
    : ["write_failed", "write_rejected", "writer_disconnected"].includes(
          row.reason
        )
      ? "Saving"
      : [
            "unavailable",
            "paused",
            "idle_suspended",
            "permission_denied",
            "incompatible",
            "partial_population",
          ].includes(row.reason)
        ? "Collection"
        : "Metrics";
export function HistoryDetails({
  open,
  onOpenChange,
  records,
  containers,
  savingError,
}: {
  savingError?: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  records: readonly DockerCoverage[];
  containers: readonly DockerContainer[];
}) {
  const merged = mergeCoverage(records);
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogTitle>History details</DialogTitle>
        <DialogDescription>
          Collection, saving and coverage for this range.
        </DialogDescription>
        <div className="max-h-88 overflow-y-auto">
          {["Collection", "Saving", "Metrics", "Events"].map((label) => {
            const items = merged.filter((row) => group(row) === label);
            return items.length ? (
              <section key={label} className="mb-4">
                <h3 className="mb-2 text-[13px]">{label}</h3>
                {items.map((row, index) => (
                  <div
                    key={`${row.reason}:${row.container_id}:${row.start_ms}`}
                    className="border-border-subtle border-b py-2 text-[12px]"
                  >
                    <div>{historyReason[row.reason] ?? row.reason}</div>
                    {label === "Saving" && index === 0 && savingError && (
                      <p className="mt-1 break-words text-[12px] text-muted-foreground">
                        {savingError}
                      </p>
                    )}
                    <div className="text-[11px] text-muted-foreground">
                      {new Date(row.start_ms).toLocaleTimeString()}–
                      {row.end_ms === null
                        ? "now"
                        : new Date(row.end_ms).toLocaleTimeString()}{" "}
                      ·{" "}
                      {row.container_id === null
                        ? "All containers"
                        : row.container_id === ""
                          ? "Engine aggregate"
                          : (containers.find((c) => c.id === row.container_id)
                              ?.name ?? row.container_id)}
                    </div>
                    <code className="text-[11px] text-muted-foreground">
                      {row.reason}
                    </code>
                  </div>
                ))}
              </section>
            ) : null;
          })}
        </div>
      </DialogContent>
    </Dialog>
  );
}
