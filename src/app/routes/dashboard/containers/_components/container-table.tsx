import type {
  DockerContainer,
  DockerRankedContainer,
} from "@core/generated/bindings";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  type MouseEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { useLocation, useNavigate } from "react-router";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuTrigger,
} from "~/components/ui/context-menu";
import { useUnits } from "~/hooks/use-units";
import {
  containerReturnState,
  isContainerDetailClick,
} from "../_lib/navigation";
import {
  type ContainerSort,
  containerPath,
  sortedContainers,
} from "../_lib/presentation";
import {
  ContainerActionMenuItem,
  containerAction,
  useContainerActions,
} from "./container-actions";

import { ContainerDataRow } from "./container-data-row";
import { ContainerTableHead } from "./container-table-head";
export const needsAttention = (row: DockerContainer) =>
  row.health === "unhealthy" || row.oom_killed || row.repeated_restarts;
export function ContainerTable({
  rows,
  sort,
  onSort,
  grouped,
  collapsed = [],
  onCollapse,
  slots = {},
  focused = null,
  onFocus,
  columns = [],
  filters,
  attention,
  historical = false,
  title = "Containers now",
  liveRows,
  summaries,
}: {
  rows: readonly DockerContainer[];
  sort: ContainerSort;
  onSort: (sort: ContainerSort) => void;
  grouped: boolean;
  collapsed?: readonly string[];
  onCollapse: (group: string) => void;
  slots?: Record<string, number>;
  focused?: string | null;
  onFocus?: (id: string | null) => void;
  columns?: readonly string[];
  filters?: ReactNode;
  attention?: ReactNode;
  historical?: boolean;
  title?: string;
  liveRows?: readonly DockerContainer[];
  summaries?: readonly DockerRankedContainer[] | undefined;
}) {
  const location = useLocation();
  const navigate = useNavigate();
  const actions = useContainerActions();
  const { bytes, rate } = useUnits();
  const summaryById = new Map(summaries?.map((row) => [row.container.id, row]));
  const scroller = useRef<HTMLDivElement>(null);
  const [hovered, setHovered] = useState(false);
  const [keyboard, setKeyboard] = useState(false);
  const [contextId, setContextId] = useState<string | null>(null);
  const [contextOpen, setContextOpen] = useState(false);
  const contextRow = (liveRows ?? (historical ? [] : rows)).find(
    (row) => row.id === contextId
  );
  const held = useRef<string[]>([]);
  const previousSort = useRef(sort);
  const paused =
    hovered || keyboard || contextOpen || actions.pendingIds.size > 0;
  const sorted = sortedContainers(rows, sort);
  if (!paused || previousSort.current !== sort)
    held.current = sorted.map((row) => row.id);
  previousSort.current = sort;
  const rank = new Map(held.current.map((id, index) => [id, index]));
  const ordered = paused
    ? [...rows].sort(
        (a, b) =>
          (rank.get(a.id) ?? Infinity) - (rank.get(b.id) ?? Infinity) ||
          a.id.localeCompare(b.id)
      )
    : sorted;
  const groups = grouped
    ? [...new Set(ordered.map((row) => row.project ?? "Standalone"))]
    : [""];
  const items: (
    | { kind: "group"; group: string; count: number }
    | { kind: "row"; row: DockerContainer }
  )[] = groups.flatMap((group) => {
    const members = grouped
      ? ordered.filter((row) => (row.project ?? "Standalone") === group)
      : ordered;
    return [
      ...(grouped
        ? [{ kind: "group" as const, group, count: members.length }]
        : []),
      ...(!grouped || !collapsed.includes(group)
        ? members.map((row) => ({ kind: "row" as const, row }))
        : []),
    ];
  });
  const virtual = useVirtualizer({
    count: items.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 44,
    overscan: 5,
    enabled: rows.length > 100,
    getItemKey: (index) => {
      const item = items[index];
      return !item
        ? index
        : item.kind === "group"
          ? `group:${item.group}`
          : item.row.id;
    },
  });
  useEffect(() => {
    const saved =
      (location.state as { tableScroll?: number } | null)?.tableScroll ??
      window.history.state?.containerTableScroll;
    if (typeof saved === "number" && scroller.current)
      scroller.current.scrollTop = saved;
  }, [location.state]);
  useEffect(() => {
    if (!focused || hovered) return;
    const index = items.findIndex(
      (item) => item.kind === "row" && item.row.id === focused
    );
    if (index < 0) return;
    if (rows.length > 100) virtual.scrollToIndex(index, { align: "auto" });
    else {
      scrollFocusedRow(scroller.current, focused);
    }
  }, [focused, hovered, items, rows.length, virtual]);
  const count = 7 + columns.length;
  const openDetail = useCallback(
    (event: MouseEvent<HTMLAnchorElement>, engine: string, id: string) => {
      if (!isContainerDetailClick(event)) return;
      event.preventDefault();
      void navigate(containerPath(engine, id), {
        state: containerReturnState(location, scroller.current),
      });
    },
    [navigate, location]
  );
  const render = (item: (typeof items)[number]) => {
    if (item.kind === "group")
      return (
        <tr key={`group:${item.group}`} className="h-11 bg-raised/40">
          <th colSpan={count} className="px-3 text-left">
            <button
              className="w-full text-left text-[12px]"
              type="button"
              aria-expanded={!collapsed.includes(item.group)}
              onClick={() => onCollapse(item.group)}
            >
              {collapsed.includes(item.group) ? "›" : "⌄"} {item.group}
              <span className="ml-2 font-normal text-muted-foreground">
                {item.count} containers
              </span>
            </button>
          </th>
        </tr>
      );
    const row = item.row;
    return (
      <ContainerDataRow
        key={row.id}
        row={row}
        summary={historical ? summaryById.get(row.id) : undefined}
        current={
          liveRows?.find((r) => r.id === row.id) ?? (historical ? null : row)
        }
        slot={slots[row.id]}
        isFocused={focused === row.id}
        onFocus={onFocus}
        failure={actions.failures[row.id]}
        openDetail={openDetail}
        historical={historical}
        columns={columns}
        bytes={bytes}
        rate={rate}
      />
    );
  };
  const virtualItems = virtual.getVirtualItems();
  const top = virtualItems[0]?.start ?? 0;
  const bottom = virtual.getTotalSize() - (virtualItems.at(-1)?.end ?? 0);
  return (
    <div className="@container">
      <div className="flex flex-wrap items-center gap-3 px-3 py-2">
        <p className="mr-auto text-[14px]">{title}</p>
        {filters}
        <span className="text-[11px] text-muted-foreground">
          {rows.length} matching containers
        </span>
        {attention}
      </div>
      <ContextMenu onOpenChange={setContextOpen}>
        <ContextMenuTrigger asChild>
          <div
            onContextMenuCapture={(event) => {
              const target =
                event.target instanceof Element
                  ? event.target.closest<HTMLElement>("tr[data-container-id]")
                  : null;
              const row = (liveRows ?? (historical ? [] : rows)).find(
                (candidate) => candidate.id === target?.dataset.containerId
              );
              if (!row || !containerAction(row)) {
                event.preventDefault();
                setContextId(null);
                return;
              }
              setContextId(row.id);
            }}
            data-container-table-scroll
            ref={scroller}
            className="max-h-88 overflow-y-scroll"
            onPointerEnter={() => setHovered(true)}
            onPointerLeave={() => {
              setHovered(false);
              onFocus?.(null);
            }}
            onFocusCapture={() => setKeyboard(true)}
            onBlurCapture={(event) => {
              if (!event.currentTarget.contains(event.relatedTarget)) {
                setKeyboard(false);
                onFocus?.(null);
              }
            }}
          >
            <table
              className="w-full table-fixed border-collapse"
              aria-label={title}
            >
              <ContainerTableHead
                sort={sort}
                onSort={onSort}
                historical={historical}
                columns={columns}
              />
              <tbody>
                {rows.length > 100 ? (
                  <>
                    {top > 0 && (
                      <tr aria-hidden>
                        <td colSpan={count} style={{ height: top }} />
                      </tr>
                    )}
                    {virtualItems.map((item) => {
                      const entry = items[item.index];
                      return entry ? render(entry) : null;
                    })}
                    {bottom > 0 && (
                      <tr aria-hidden>
                        <td colSpan={count} style={{ height: bottom }} />
                      </tr>
                    )}
                  </>
                ) : (
                  items.map(render)
                )}
              </tbody>
            </table>
            {!rows.length && (
              <p className="px-3 py-8 text-[12px] text-muted-foreground">
                No containers match this view.
              </p>
            )}
          </div>
        </ContextMenuTrigger>
        {contextRow && (
          <ContextMenuContent>
            <ContainerActionMenuItem row={contextRow} />
          </ContextMenuContent>
        )}
      </ContextMenu>
    </div>
  );
}

function scrollFocusedRow(scroller: HTMLDivElement | null, focused: string) {
  const row = scroller?.querySelector(
    `[data-container-id="${CSS.escape(focused)}"]`
  );
  if (row && scroller) {
    const bounds = row.getBoundingClientRect();
    const view = scroller.getBoundingClientRect();
    if (bounds.top < view.top + 32 || bounds.bottom > view.bottom)
      row.scrollIntoView({ block: "nearest" });
  }
}
