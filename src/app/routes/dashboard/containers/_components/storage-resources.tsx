import type {
  DockerStorageResource,
  DockerStorageSnapshot,
} from "@core/generated/bindings";
import type { Dispatch, SetStateAction } from "react";
import { SectionCard } from "~/components/section-card";
import { Button } from "~/components/ui/button";
import {
  type StorageSort,
  storageResourceKey,
} from "../_lib/storage-presentation";
import type { CleanupFlow } from "./docker-cleanup-flow";
import { StorageResourceRow } from "./storage-resource-row";
export function StorageResources({
  rows,
  chosen,
  error,
  connected,
  snapshot,
  flow,
  refreshing,
  fetching,
  groups,
  grouped,
  pageRows,
  selected,
  setSelected,
  sort,
  setSort,
  currentPage,
  setPage,
}: {
  rows: readonly DockerStorageResource[];
  chosen: readonly DockerStorageResource[];
  error: string | null | undefined;
  connected: boolean;
  snapshot: DockerStorageSnapshot;
  flow: CleanupFlow;
  refreshing: boolean;
  fetching: boolean;
  groups: readonly string[];
  grouped: boolean;
  pageRows: readonly DockerStorageResource[];
  selected: Set<string>;
  setSelected: Dispatch<SetStateAction<Set<string>>>;
  sort: StorageSort;
  setSort: (value: StorageSort) => void;
  currentPage: number;
  setPage: (page: number) => void;
}) {
  return (
    <SectionCard accent="disk" title="Storage resources" hiddenTitle flush>
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
            fetching
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
              <th className="px-3 py-2 text-left font-normal">Eligibility</th>
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
                    !grouped || (row.project ?? "No Compose project") === group
                )
                .map((row) => (
                  <StorageResourceRow
                    key={storageResourceKey(row)}
                    row={row}
                    selected={selected.has(storageResourceKey(row))}
                    disabled={
                      Boolean(error) ||
                      !connected ||
                      !snapshot.complete ||
                      flow.busy
                    }
                    selectionFull={chosen.length >= 128}
                    onToggle={(checked) =>
                      setSelected((previous) => {
                        const next = new Set(previous);
                        if (checked === true) next.add(storageResourceKey(row));
                        else next.delete(storageResourceKey(row));
                        return next;
                      })
                    }
                  />
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
  );
}
