import type { DockerSnapshot } from "@core/generated/bindings";
import type { SetURLSearchParams } from "react-router";
import { brushScopeProps } from "~/components/brush-overlay";
import { SectionCard } from "~/components/section-card";
import { SelectionChip } from "~/components/selection-chip";
import { useBrushRange } from "~/stores/brush-store";
import type { useContainerComparison } from "../_hooks/use-container-comparison";
import { type ContainerSort, filterContainers } from "../_lib/presentation";
import type { ContainerRouteState } from "../_lib/route-state";
import { ContainerFilters } from "./container-filters";
import { ContainerTable, needsAttention } from "./container-table";

export function ContainerResults({
  snapshot,
  route,
  chart,
  set,
  setParams,
  onSort,
}: {
  snapshot: DockerSnapshot;
  route: ContainerRouteState;
  chart: ReturnType<typeof useContainerComparison>;
  set: (key: string, value: string | null) => void;
  setParams: SetURLSearchParams;
  onSort: (sort: ContainerSort) => void;
}) {
  const {
    project,
    query,
    state,
    sort,
    grouped,
    columns,
    collapsed,
    end,
    from,
    to,
  } = route;
  const {
    ranking,
    selectedComparison,
    slots,
    pinnedFocus,
    focused,
    setFocused,
  } = chart;
  const engine = snapshot.engine?.id ?? "";
  const selection = useBrushRange();
  const scopeRows = snapshot.containers.filter(
    (row) => project === null || (row.project ?? "") === project
  );
  const historical = end !== null || selection !== null;
  const tableRanking = selection
    ? selectedComparison.data?.ranking.filter(
        (row) => row.container.engine_id === engine
      )
    : ranking;
  const tableRows = historical
    ? (tableRanking ?? []).map((r) => ({
        ...r.container,
        lifecycle: r.last_lifecycle ?? ("unknown" as const),
        metrics: {
          ...r.container.metrics,
          cpu_pct: r.peak_cpu_pct,
          working_set_bytes: r.peak_working_set_bytes,
        },
      }))
    : scopeRows;
  const shown = filterContainers(tableRows, query, state);
  const hiddenAttention = tableRows.filter(
    (row) =>
      needsAttention(row) && !shown.some((visible) => visible.id === row.id)
  );
  const filters = (
    <ContainerFilters
      query={query}
      state={state}
      grouped={grouped}
      columns={columns}
      set={set}
    />
  );
  return (
    <div {...brushScopeProps}>
      {selection && selectedComparison.error && (
        <p role="alert" className="text-[11px] text-muted-foreground">
          Selected history could not be read: {selectedComparison.error.message}
        </p>
      )}
      <SectionCard accent="cpu" title="Containers now" hiddenTitle flush>
        <ContainerTable
          liveRows={snapshot.containers}
          rows={shown}
          sort={sort}
          onSort={(value) => {
            onSort(value);
          }}
          grouped={grouped}
          columns={columns}
          collapsed={collapsed}
          onCollapse={(group) =>
            setParams(
              (old) => {
                const next = new URLSearchParams(old);
                const values = next.getAll("collapsed");
                next.delete("collapsed");
                for (const v of values.includes(group)
                  ? values.filter((v) => v !== group)
                  : [...values, group])
                  next.append("collapsed", v);
                return next;
              },
              { replace: true }
            )
          }
          slots={slots}
          focused={pinnedFocus ?? focused}
          onFocus={setFocused}
          filters={
            <>
              {selection && <SelectionChip range={selection} />}
              {filters}
            </>
          }
          historical={historical}
          summaries={historical ? tableRanking : undefined}
          title={
            historical
              ? `Containers · ${new Date(selection?.fromMs ?? from).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}–${new Date(selection?.toMs ?? to).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
              : "Containers now"
          }
          attention={
            hiddenAttention.length ? (
              <button
                type="button"
                className="text-[11px] underline"
                onClick={() => {
                  setParams(
                    (old) => {
                      const next = new URLSearchParams(old);
                      next.delete("q");
                      next.delete("state");
                      const targetGroup =
                        hiddenAttention[0]?.project ?? "Standalone";
                      const hiddenGroups = next
                        .getAll("collapsed")
                        .filter((group) => group !== targetGroup);
                      next.delete("collapsed");
                      for (const group of hiddenGroups)
                        next.append("collapsed", group);
                      return next;
                    },
                    { replace: true }
                  );
                  setFocused(hiddenAttention[0]?.id ?? null);
                }}
              >
                {hiddenAttention.length} need attention
              </button>
            ) : undefined
          }
        />
      </SectionCard>
    </div>
  );
}
