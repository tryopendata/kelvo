import { formatBytes } from "@core/format";
import { dockerKeys } from "@core/query-keys";
import { unwrap } from "@core/transport";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { memo, useMemo, useState } from "react";
import { SearchField } from "~/components/search-field";
import { SectionCard } from "~/components/section-card";
import { SegmentedControl } from "~/components/segmented-control";
import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { useTransport } from "~/lib/transport-context";
import {
  type StorageSort,
  storageAgeLabel,
  storageKindLabel,
  storageResourceKey,
  storageRows,
} from "../_lib/storage-presentation";
import type { CleanupFlow } from "./docker-cleanup-flow";

export const DockerStorage = memo(function DockerStorage({
  engine,
  generation,
  connected,
  flow,
}: {
  engine: string;
  generation: number;
  connected: boolean;
  flow: CleanupFlow;
}) {
  const transport = useTransport();
  const client = useQueryClient();
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState("all");
  const [age, setAge] = useState("7");
  const [sort, setSort] = useState<StorageSort>("size");
  const [grouped, setGrouped] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [page, setPage] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshError, setRefreshError] = useState<string | null>(null);
  const storage = useQuery({
    queryKey: dockerKeys.storage(engine, generation),
    enabled: Boolean(engine) && connected,
    queryFn: () => unwrap(transport.dockerStorageScan(false)),
    staleTime: 300_000,
    refetchOnWindowFocus: false,
    refetchOnReconnect: false,
    retry: false,
  });
  const snapshot = storage.data?.snapshot;
  const rows = useMemo(
    () => storageRows(snapshot?.resources ?? [], query, category, age, sort),
    [snapshot, query, category, age, sort]
  );
  const chosen =
    snapshot?.resources.filter(
      (row) => selected.has(storageResourceKey(row)) && row.can_delete
    ) ?? [];
  const currentPage = Math.min(
    page,
    Math.max(0, Math.ceil(rows.length / 32) - 1)
  );
  const pageRows = rows.slice(currentPage * 32, (currentPage + 1) * 32);
  const groups = grouped
    ? [...new Set(pageRows.map((row) => row.project ?? "No Compose project"))]
    : [""];
  const refresh = async () => {
    setRefreshing(true);
    setRefreshError(null);
    try {
      client.setQueryData(
        dockerKeys.storage(engine, generation),
        await unwrap(transport.dockerStorageScan(true))
      );
    } catch (e) {
      setRefreshError(String(e));
    } finally {
      setRefreshing(false);
    }
  };
  const error = refreshError ?? storage.data?.error ?? storage.error?.message;
  return (
    <section aria-label="Docker storage" className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="font-[590] text-[16px]">Docker-managed storage</h2>
          <p className="mt-1 text-[11px] text-muted-foreground">
            {snapshot
              ? `Measured ${new Date(snapshot.observed_ms).toLocaleString()}`
              : "Scan images, writable layers, volumes and daemon build cache on demand."}
          </p>
        </div>
        <Button
          size="sm"
          variant="outline"
          disabled={!connected || storage.isFetching || refreshing || flow.busy}
          onClick={() => void refresh()}
        >
          {storage.isFetching || refreshing ? "Scanning…" : "Refresh storage"}
        </Button>
      </div>
      {!connected && (
        <p role="status" className="text-[12px] text-muted-foreground">
          Storage scanning requires a connected engine. Resume monitoring if it
          is paused.
        </p>
      )}
      {error && (
        <p role="alert" className="text-[12px] text-destructive">
          {error}
          {snapshot ? " The previous measurement is retained." : ""}
        </p>
      )}
      {storage.isPending && connected && (
        <p role="status" className="text-[12px] text-muted-foreground">
          Reading Docker storage…
        </p>
      )}
      {snapshot && (
        <>
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
            Shared image and cache layers make sizes non-additive. VM
            allocation, logs, bind-mounted host files and other Buildx builders
            may use additional space. Unreferenced does not mean disposable.
          </p>
          {!snapshot.complete && (
            <p role="status" className="text-[12px] text-muted-foreground">
              Incomplete inventory. Cleanup is unavailable until references can
              be verified.
            </p>
          )}
          {snapshot.warnings.map((warning) => (
            <p key={warning} className="text-[12px] text-muted-foreground">
              {warning}
            </p>
          ))}
          <div className="flex flex-wrap items-center gap-3">
            <SearchField
              value={query}
              onChange={(value) => {
                setQuery(value);
                setPage(0);
              }}
              placeholder="Resource, project or database"
              label="Search Docker storage resources"
            />
            <Select
              value={category}
              onValueChange={(value) => {
                setCategory(value);
                setPage(0);
              }}
            >
              <SelectTrigger aria-label="Storage review filter" size="sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">All resources</SelectItem>
                <SelectItem value="container">Stopped containers</SelectItem>
                <SelectItem value="image">Unused images</SelectItem>
                <SelectItem value="volume">Unreferenced volumes</SelectItem>
                <SelectItem value="build_cache">Old build cache</SelectItem>
              </SelectContent>
            </Select>
            <Select
              value={age}
              onValueChange={(value) => {
                setAge(value);
                setPage(0);
              }}
            >
              <SelectTrigger aria-label="Stopped or last-used age" size="sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="1">Older than 1 day</SelectItem>
                <SelectItem value="7">Older than 7 days</SelectItem>
                <SelectItem value="30">Older than 30 days</SelectItem>
                <SelectItem value="all">All ages, including unknown</SelectItem>
                <SelectItem value="unknown">Unknown ages</SelectItem>
              </SelectContent>
            </Select>
            <SegmentedControl
              options={[
                { value: "flat", label: "Flat" },
                { value: "grouped", label: "By project" },
              ]}
              value={grouped ? "grouped" : "flat"}
              onChange={(value) => setGrouped(value === "grouped")}
              ariaLabel="Storage grouping"
            />
          </div>
          <p className="text-[11px] text-muted-foreground">
            Age filters use stopped time for containers and last use for cache.
            Images and volumes have no reliable unused age.
          </p>
          <SectionCard
            accent="disk"
            title="Storage resources"
            hiddenTitle
            flush
          >
            <div className="flex flex-wrap items-center justify-between gap-2 px-3 py-2 text-[11px] text-muted-foreground">
              <span>
                {rows.length} resources shown · {chosen.length} selected
              </span>
              <Button
                size="sm"
                variant="outline"
                disabled={
                  Boolean(error) ||
                  !connected ||
                  !snapshot.complete ||
                  !chosen.length ||
                  flow.busy ||
                  refreshing ||
                  storage.isFetching
                }
                onClick={() =>
                  void flow.prepare(
                    chosen.map((row) => ({
                      kind: row.kind,
                      id: row.id,
                      action: "delete",
                    }))
                  )
                }
              >
                Review cleanup
              </Button>
            </div>
            <div className="overflow-x-auto">
              <table
                aria-label="Storage resources"
                className="w-full border-collapse text-[12px]"
              >
                <thead className="border-border border-y text-[11px] text-muted-foreground">
                  <tr>
                    {(
                      [
                        ["Resource / type", "name"],
                        ["Size", "size"],
                        ["References", "references"],
                        ["Age evidence", "age"],
                      ] as const
                    ).map(([label, key]) => (
                      <th
                        key={key}
                        className="px-3 py-2 text-left font-normal"
                        aria-sort={
                          sort === key
                            ? key === "name"
                              ? "ascending"
                              : "descending"
                            : "none"
                        }
                      >
                        <button
                          type="button"
                          className="rounded-control focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                          onClick={() => setSort(key)}
                        >
                          {label}
                          {sort === key ? (key === "name" ? " ↑" : " ↓") : ""}
                        </button>
                      </th>
                    ))}
                    <th className="px-3 py-2 text-left font-normal">
                      Eligibility
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {groups.flatMap((group) => [
                    ...(grouped
                      ? [
                          <tr key={`group:${group}`} className="bg-raised/40">
                            <th
                              colSpan={5}
                              className="px-3 py-2 text-left font-normal"
                            >
                              {group}
                            </th>
                          </tr>,
                        ]
                      : []),
                    ...pageRows
                      .filter(
                        (row) =>
                          !grouped ||
                          (row.project ?? "No Compose project") === group
                      )
                      .map((row) => (
                        <tr
                          key={storageResourceKey(row)}
                          className="border-border-subtle border-t align-top hover:bg-selected/50"
                        >
                          <td className="max-w-72 px-3 py-2">
                            <label
                              htmlFor={`storage-${storageResourceKey(row)}`}
                              className="flex items-start gap-2"
                            >
                              <Checkbox
                                id={`storage-${storageResourceKey(row)}`}
                                aria-label={`Select ${row.name}`}
                                checked={
                                  selected.has(storageResourceKey(row)) &&
                                  row.can_delete
                                }
                                disabled={
                                  Boolean(error) ||
                                  !connected ||
                                  !snapshot.complete ||
                                  !row.can_delete ||
                                  flow.busy ||
                                  (!selected.has(storageResourceKey(row)) &&
                                    chosen.length >= 128)
                                }
                                onCheckedChange={(checked) =>
                                  setSelected((previous) => {
                                    const next = new Set(previous);
                                    if (checked === true)
                                      next.add(storageResourceKey(row));
                                    else next.delete(storageResourceKey(row));
                                    return next;
                                  })
                                }
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
                                Likely {row.database} database · inferred from
                                image
                              </p>
                            )}
                            {row.kind === "volume" &&
                              !row.references.length &&
                              !row.database && (
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
                                    <li
                                      key={`${mount.kind}:${mount.source}:${mount.destination}`}
                                    >
                                      {mount.kind} ·{" "}
                                      {mount.name ??
                                        mount.source ??
                                        "Unknown source"}{" "}
                                      →{" "}
                                      {mount.destination ??
                                        "Unknown destination"}
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
                              row.references.join(", ") ||
                              "No container references"
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
                      )),
                  ])}
                </tbody>
              </table>
            </div>
            {!rows.length && (
              <p className="px-3 py-8 text-[12px] text-muted-foreground">
                No resources match this view.
              </p>
            )}
            {rows.length > 32 && (
              <div className="flex items-center justify-end gap-2 px-3 py-2 text-[11px]">
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={currentPage === 0}
                  onClick={() => setPage(currentPage - 1)}
                >
                  Previous resources
                </Button>
                <span className="figures">
                  {currentPage * 32 + 1}–
                  {Math.min(rows.length, (currentPage + 1) * 32)}
                </span>
                <Button
                  size="sm"
                  variant="ghost"
                  disabled={(currentPage + 1) * 32 >= rows.length}
                  onClick={() => setPage(currentPage + 1)}
                >
                  Next resources
                </Button>
              </div>
            )}
          </SectionCard>
        </>
      )}
    </section>
  );
});
