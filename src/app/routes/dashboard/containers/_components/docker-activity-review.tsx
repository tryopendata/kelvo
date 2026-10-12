import type {
  DockerContainer,
  DockerStorageView,
} from "@core/generated/bindings";
import { unwrap } from "@core/transport";
import { useState } from "react";
import { Link } from "react-router";
import { Button } from "~/components/ui/button";
import { useTransport } from "~/lib/transport-context";
import { containerPath } from "../_lib/presentation";
import type { CleanupFlow } from "./docker-cleanup-flow";

export function DockerActivityReview({
  rows,
  connected,
  flow,
}: {
  rows: readonly DockerContainer[];
  connected: boolean;
  flow: CleanupFlow;
}) {
  const transport = useTransport();
  const [view, setView] = useState<DockerStorageView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const flagged = rows.filter(
    (row) =>
      row.health === "unhealthy" || row.oom_killed || row.repeated_restarts
  );
  const inspect = async () => {
    setBusy(true);
    setError(null);
    try {
      setView(await unwrap(transport.dockerStorageScan(false)));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <section
      aria-label="Workload review"
      className="border-border-subtle border-y py-3 text-[12px]"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h2 className="font-[590]">Workload review</h2>
        <Button
          size="sm"
          variant="outline"
          disabled={!connected || busy || flow.busy}
          onClick={() => void inspect()}
        >
          {busy ? "Checking running containers…" : "Review running containers"}
        </Button>
      </div>
      {flagged.length ? (
        <ul className="mt-2 space-y-1">
          {flagged.slice(0, 8).map((row) => (
            <li key={row.id}>
              <Link
                className="rounded-control underline underline-offset-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                to={containerPath(row.engine_id, row.id)}
              >
                Review {row.name}
              </Link>{" "}
              ·{" "}
              {[
                row.health === "unhealthy" ? "Unhealthy" : null,
                row.oom_killed ? "OOM recorded" : null,
                row.repeated_restarts ? "Repeated restarts observed" : null,
              ]
                .filter(Boolean)
                .join(" · ")}
            </li>
          ))}
        </ul>
      ) : (
        <p className="mt-2 text-muted-foreground">
          No unhealthy, OOM or repeated-restart evidence in this inventory.
        </p>
      )}
      {flagged.length > 8 && (
        <p className="mt-1 text-muted-foreground">
          {flagged.length - 8} more workloads have review evidence; inspect the
          table below.
        </p>
      )}
      <p className="mt-2 text-[11px] text-muted-foreground">
        Use CPU or Memory column headings below to rank measured workloads. Low
        usage alone does not establish abandonment.
      </p>
      {(error || view?.error) && (
        <p role="alert" className="mt-2 text-destructive">
          {error ?? view?.error}
        </p>
      )}
      {view?.snapshot && (
        <ul className="mt-2 max-h-64 space-y-2 overflow-y-auto">
          {rows
            .filter((row) => row.lifecycle === "running")
            .map((row) => {
              const resource = view.snapshot?.resources.find(
                (item) => item.kind === "container" && item.id === row.id
              );
              return (
                <li
                  key={row.id}
                  className="flex items-center justify-between gap-3"
                >
                  <span>
                    {row.name}
                    {(!resource?.can_stop || resource.auto_remove) && (
                      <span className="block text-[11px] text-muted-foreground">
                        {resource?.auto_remove
                          ? "Stop unavailable: AutoRemove can delete this container and its anonymous volumes."
                          : (resource?.reason ?? "Stop metadata unavailable")}
                      </span>
                    )}
                  </span>
                  <Button
                    size="sm"
                    variant="outline"
                    aria-label={`Stop ${row.name}`}
                    disabled={
                      Boolean(error || view.error) ||
                      !connected ||
                      flow.busy ||
                      !resource?.can_stop ||
                      resource.auto_remove !== false ||
                      !view.snapshot?.complete
                    }
                    onClick={() =>
                      void flow.prepare([
                        { kind: "container", id: row.id, action: "stop" },
                      ])
                    }
                  >
                    Stop…
                  </Button>
                </li>
              );
            })}
        </ul>
      )}
    </section>
  );
}
