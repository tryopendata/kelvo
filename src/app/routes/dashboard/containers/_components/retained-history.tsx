import { dockerKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { memo, useState } from "react";
import { Link, useLocation, useSearchParams } from "react-router";
import { SearchField } from "~/components/search-field";
import { Button } from "~/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { useTransport } from "~/lib/transport-context";
import { useHostId } from "~/stores/host-store";
import { useDockerHistory } from "../_hooks/use-docker";
import { containerPath, lifecycleLabel } from "../_lib/presentation";
import { ContainerHistory } from "./container-history";
import {
  DOCKER_RANGES,
  type DockerRange,
  HistoryControls,
} from "./history-controls";

export const RetainedHistory = memo(function RetainedHistory({
  enabled,
  generation,
}: {
  enabled: boolean;
  generation: number;
}) {
  const [params, setParams] = useSearchParams();
  const open = !enabled || params.get("retained") === "1";
  return (
    <section
      aria-label="Retained Docker history"
      className="border-border-subtle border-y py-3"
    >
      {enabled ? (
        <Button
          variant="ghost"
          size="sm"
          aria-expanded={open}
          onClick={() =>
            setParams(
              (previous) => {
                const next = new URLSearchParams(previous);
                if (open) next.delete("retained");
                else next.set("retained", "1");
                return next;
              },
              { replace: true }
            )
          }
        >
          Browse retained history
        </Button>
      ) : (
        <h2 className="text-[14px]">Retained history</h2>
      )}
      {open && <RetainedBrowser generation={generation} />}
    </section>
  );
});

function RetainedBrowser({ generation }: { generation: number }) {
  const transport = useTransport();
  const host = useHostId();
  const location = useLocation();
  const [params, setParams] = useSearchParams();
  const [now] = useState(() => Date.now());
  const selected = params.get("retainedEngine");
  const search = params.get("retainedQuery") ?? "";
  const offset = Math.max(0, Number(params.get("retainedOffset")) || 0);
  const range = (params.get("retainedRange") ?? "1h") as DockerRange;
  const validRange = range in DOCKER_RANGES ? range : "1h";
  const end = params.has("retainedEnd")
    ? Number(params.get("retainedEnd"))
    : null;
  const set = (key: string, value: string | null, resetPage = false) =>
    setParams(
      (previous) => {
        const next = new URLSearchParams(previous);
        next.set("retained", "1");
        if (value === null) next.delete(key);
        else next.set(key, value);
        if (resetPage) next.delete("retainedOffset");
        return next;
      },
      { replace: true }
    );
  const catalog = useQuery({
    queryKey: dockerKeys.retained(host, null, "", 0, generation),
    queryFn: () =>
      unwrap(
        transport.dockerRetained({
          engine_id: null,
          search: "",
          offset: 0,
          limit: 1,
        })
      ),
  });
  const engines = catalog.isError ? [] : (catalog.data?.engines ?? []);
  const engine =
    engines.find((item) => item.id === selected)?.id ?? engines[0]?.id ?? "";
  const rows = useQuery({
    queryKey: dockerKeys.retained(host, engine, search, offset, generation),
    enabled: Boolean(engine),
    queryFn: () =>
      unwrap(
        transport.dockerRetained({
          engine_id: engine,
          search,
          offset,
          limit: 32,
        })
      ),
  });
  const to = end ?? now;
  const from = to - DOCKER_RANGES[validRange];
  const history = useDockerHistory(engine, null, from, to, generation);
  return (
    <div className="mt-3 flex flex-col gap-3">
      <p className="text-[12px] text-muted-foreground">
        Browse saved engines and containers without enabling collection or
        reconnecting an engine. Values and states are last observed.
      </p>
      {catalog.error && (
        <p role="alert">
          Retained catalog unavailable: {catalog.error.message}
        </p>
      )}
      {catalog.isPending && <p role="status">Loading retained engines…</p>}
      {catalog.isSuccess && engines.length === 0 && (
        <p className="text-[12px] text-muted-foreground">
          No retained Docker history yet.
        </p>
      )}
      {engine && (
        <>
          <div className="flex flex-wrap items-center gap-3">
            <Select
              value={engine}
              onValueChange={(id) => set("retainedEngine", id, true)}
            >
              <SelectTrigger aria-label="Retained engine" size="sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {engines.map((item) => (
                  <SelectItem key={item.id} value={item.id}>
                    {item.runtime} · {item.id}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <HistoryControls
              range={validRange}
              end={end}
              onRange={(value) => set("retainedRange", value)}
              onEnd={(value) =>
                set("retainedEnd", value === null ? null : String(value))
              }
              now={now}
            />
          </div>
          {history.error && (
            <p role="alert">
              Retained measurements unavailable: {history.error.message}
            </p>
          )}
          <div className="grid min-w-0 gap-3 lg:grid-cols-2">
            <ContainerHistory
              aggregate
              compact
              page={history.data}
              metric="cpu_pct"
              title="Retained workload · CPU"
              from={from}
              to={to}
            />
            <ContainerHistory
              aggregate
              compact
              page={history.data}
              metric="working_set_bytes"
              title="Retained workload · Memory"
              from={from}
              to={to}
            />
          </div>
          <SearchField
            value={search}
            onChange={(value) => set("retainedQuery", value, true)}
            placeholder="Find retained containers"
          />
          {rows.error && (
            <p role="alert">
              Retained containers unavailable: {rows.error.message}
            </p>
          )}
          {rows.isPending && <p role="status">Loading retained containers…</p>}
          {rows.isSuccess && (
            <>
              <p className="text-[11px] text-muted-foreground">
                {rows.data.total} retained{" "}
                {rows.data.total === 1 ? "container" : "containers"} · last
                observed state
              </p>
              {rows.data.containers.length === 0 && (
                <p className="text-[12px] text-muted-foreground">
                  No retained containers match this search.
                </p>
              )}
              <ul
                aria-label="Retained containers"
                className="divide-y divide-border-subtle"
              >
                {rows.data.containers.map((row) => (
                  <li
                    key={`${row.engine_id}:${row.id}`}
                    className="py-2 text-[12px]"
                  >
                    <Link
                      className="rounded-control text-link underline-offset-4 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      to={containerPath(row.engine_id, row.id)}
                      state={{ back: `${location.pathname}${location.search}` }}
                    >
                      {row.name}
                    </Link>
                    <span className="ml-3 text-muted-foreground">
                      {lifecycleLabel[row.lifecycle]} ·{" "}
                      {new Date(row.last_seen_ms).toLocaleString()}
                    </span>
                    <div className="truncate text-[11px] text-muted-foreground">
                      {row.image}
                    </div>
                  </li>
                ))}
              </ul>
              <div className="flex gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  disabled={offset === 0}
                  onClick={() =>
                    set("retainedOffset", String(Math.max(0, offset - 32)))
                  }
                >
                  Previous retained containers
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={rows.data.next_offset === null}
                  onClick={() =>
                    set("retainedOffset", String(rows.data.next_offset))
                  }
                >
                  Next retained containers
                </Button>
              </div>
            </>
          )}
        </>
      )}
    </div>
  );
}
