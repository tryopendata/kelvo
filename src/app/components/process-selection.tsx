import {
  holdProcessRows,
  type ProcessSelection,
  type ProcessTarget,
  type SelectableProcessRow,
  selectProcessRow,
  targetKey,
  uniqueTargets,
} from "@core/process-selection";
import {
  type ComponentProps,
  createContext,
  type KeyboardEvent,
  type ReactNode,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { ProcessRowActions, QuitDialog } from "~/components/process-actions";
import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "~/components/ui/context-menu";
import { useProcessDrag } from "~/hooks/use-process-drag";
import {
  type QuitFlow,
  quitExclusion,
  useQuitFlow,
} from "~/hooks/use-quit-flow";
import { cn } from "~/lib/utils";
import { useHostId } from "~/stores/host-store";

const EMPTY: ProcessSelection = { targets: [], anchor: null };
/** The selection toolbar, shown below the rows only while something is selected. */
export const SELECTION_BAR_PX = 40;
/** Selection is neutral; module accents stay reserved for measured data. */
const NEUTRAL_CHECKBOX =
  "data-[state=checked]:border-border-strong data-[state=checked]:bg-selected data-[state=checked]:text-foreground dark:data-[state=checked]:bg-selected";
interface SelectionContext {
  rows: readonly SelectableProcessRow[];
  selected: readonly ProcessTarget[];
  allTargets: readonly ProcessTarget[];
  keys: ReadonlySet<string>;
  focused: string | null;
  held: boolean;
  flow: QuitFlow;
  selectAll: (clear?: boolean) => void;
  select: (
    key: string,
    modifiers?: { range?: boolean; additive?: boolean; toggle?: boolean }
  ) => void;
  keyDown: (event: KeyboardEvent, key: string) => void;
  focus: (key: string) => void;
  reveal: { current: ((key: string) => void) | null };
  dragStart: (key: string, additive: boolean) => void;
  dragOver: (key: string) => void;
  dragging: { current: boolean };
}
const Selection = createContext<SelectionContext | null>(null);
export const useProcessSelection = () => useContext(Selection);

export interface ProcessSelectionSurfaceProps {
  rows: readonly SelectableProcessRow[];
  /** All search matches, including groups behind Show all. */
  allTargets: readonly ProcessTarget[];
  scopeKey?: string;
  orderKey?: string;
  /** Available height for a full-window table, including toolbar/results. */
  height?: number;
  children: ReactNode;
  onHoldChange?: (held: boolean) => void;
}

/** One view-scoped owner for selection, ordering, review and dispatch. */
export function ProcessSelectionSurface(props: ProcessSelectionSurfaceProps) {
  const host = useHostId();
  return (
    <SelectionSession key={`${host}:${props.scopeKey ?? "live"}`} {...props} />
  );
}

function SelectionSession({
  rows,
  allTargets,
  orderKey = "",
  height,
  children,
  onHoldChange,
}: ProcessSelectionSurfaceProps) {
  const [state, setState] = useState<ProcessSelection>(EMPTY);
  const [only, setOnly] = useState(false);
  const [focused, setFocused] = useState<string | null>(null);
  const [interacting, setInteracting] = useState(false);
  const [focusWithin, setFocusWithin] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const reveal = useRef<((key: string) => void) | null>(null);
  const flow = useQuitFlow();
  const held =
    interacting ||
    focusWithin ||
    state.targets.length > 0 ||
    flow.pending !== null;
  useEffect(() => {
    onHoldChange?.(held);
  }, [held, onHoldChange]);
  const order = useRef<{ key: string; keys: string[] }>({
    key: orderKey,
    keys: [],
  });
  const ordered = useMemo(() => {
    const prior =
      held && order.current.key === orderKey ? order.current.keys : [];
    const next = holdProcessRows(rows, prior);
    order.current = { key: orderKey, keys: next.map((r) => r.key) };
    return next;
  }, [rows, held, orderKey]);
  const previousOrderKey = useRef(orderKey);
  useEffect(() => {
    if (previousOrderKey.current !== orderKey) {
      previousOrderKey.current = orderKey;
      setState((s) => ({ ...s, anchor: null }));
    }
  }, [orderKey]);
  // Refresh metadata only for exact identities. Missing search/history rows are
  // retained as last observed targets, not assumed dead or replaced by PID.
  const latest = new Map(allTargets.map((p) => [targetKey(p), p]));
  const selected = state.targets.map((p) => latest.get(targetKey(p)) ?? p);
  useEffect(() => {
    const latest = new Map(allTargets.map((p) => [targetKey(p), p]));
    setState((previous) => {
      let changed = false;
      const targets = previous.targets.map((p) => {
        const now = latest.get(targetKey(p));
        if (
          !now ||
          (p.name === now.name &&
            p.refusal === now.refusal &&
            p.running === now.running)
        )
          return p;
        changed = true;
        return now;
      });
      return changed ? { ...previous, targets } : previous;
    });
  }, [allTargets]);
  const keys = new Set(selected.map(targetKey));
  const selectedRows = selected.map((p) => ({
    key: targetKey(p),
    label: `${p.name} (${p.pid})`,
    targets: [p],
  }));
  const activeRows = only ? selectedRows : ordered;
  const selectable = activeRows.filter((r) =>
    r.targets.some((p) => p.running !== false)
  );
  const visibleKeys = new Set(rows.flatMap((r) => r.targets.map(targetKey)));
  const hidden = selected.filter((p) => !visibleKeys.has(targetKey(p))).length;
  const select: SelectionContext["select"] = (key, modifiers) => {
    if (
      !activeRows
        .find((r) => r.key === key)
        ?.targets.some((p) => p.running !== false)
    )
      return;
    setFocused(key);
    setState((s) => selectProcessRow(s, activeRows, key, modifiers));
  };
  const focus = (key: string) => {
    setFocused(key);
    if (!only) reveal.current?.(key);
    // Virtual rows mount after scrollToIndex. Retry only for this gesture.
    const find = () =>
      [
        ...(root.current?.querySelectorAll<HTMLElement>("[data-process-key]") ??
          []),
      ].find((el) => el.dataset.processKey === key);
    const el = find();
    if (el) {
      el.focus({ preventScroll: true });
      el.scrollIntoView?.({ block: "nearest" });
    } else requestAnimationFrame(() => find()?.focus({ preventScroll: true }));
  };
  const selectAll = (clear = false) =>
    setState({
      targets: clear
        ? []
        : uniqueTargets(
            (only ? selected : allTargets).filter((p) => p.running !== false)
          ),
      anchor: null,
    });
  const keyDown: SelectionContext["keyDown"] = (e, key) => {
    if (
      (e.target instanceof HTMLElement &&
        e.target.closest("input,textarea,button:not([role=checkbox])")) ||
      (e.key === " " &&
        e.target instanceof HTMLElement &&
        e.target.closest("[role=checkbox]"))
    )
      return;
    if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      e.preventDefault();
      const rect = e.currentTarget.getBoundingClientRect();
      e.currentTarget.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          clientX: rect.left + 24,
          clientY: rect.bottom,
        })
      );
      return;
    }
    if (e.metaKey && e.key.toLowerCase() === "a") {
      e.preventDefault();
      selectAll();
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      setState(EMPTY);
      setOnly(false);
      return;
    }
    if (e.key === " ") {
      e.preventDefault();
      select(key, { toggle: true, range: e.shiftKey });
      return;
    }
    if (
      !["ArrowUp", "ArrowDown", "Home", "End", "PageUp", "PageDown"].includes(
        e.key
      )
    )
      return;
    e.preventDefault();
    const at = Math.max(
      0,
      selectable.findIndex((r) => r.key === key)
    );
    const step = e.key.startsWith("Page")
      ? Math.max(1, Math.floor((root.current?.clientHeight ?? 290) / 29) - 2)
      : 1;
    const next =
      e.key === "Home"
        ? 0
        : e.key === "End"
          ? selectable.length - 1
          : at + (e.key === "ArrowUp" || e.key === "PageUp" ? -step : step);
    const row = selectable[Math.max(0, Math.min(selectable.length - 1, next))];
    if (!row) return;
    if (!e.metaKey || e.shiftKey)
      select(row.key, { range: e.shiftKey, additive: e.metaKey });
    focus(row.key);
  };
  const dragging = useRef(false);
  const dragBase = useRef<ProcessSelection>(EMPTY);
  const lastDragKey = useRef<string | null>(null);
  const ctx: SelectionContext = {
    rows: activeRows,
    selected,
    allTargets: only ? selected : allTargets,
    keys,
    focused: selectable.some((r) => r.key === focused)
      ? focused
      : (selectable[0]?.key ?? null),
    held,
    flow,
    select,
    selectAll,
    keyDown,
    focus,
    reveal,
    dragging,
    dragStart: (key, additive) => {
      dragging.current = true;
      lastDragKey.current = key;
      dragBase.current = {
        targets: additive ? state.targets : [],
        anchor: key,
      };
      select(key, { additive });
    },
    dragOver: (key) => {
      if (!dragging.current || lastDragKey.current === key) return;
      lastDragKey.current = key;
      setFocused(key);
      setState(
        selectProcessRow(dragBase.current, activeRows, key, {
          range: true,
          additive: true,
        })
      );
    },
  };
  useProcessDrag(root, dragging, ctx.dragOver);
  const eligible = selected.filter((p) => quitExclusion(p) === null).length;
  return (
    <Selection.Provider value={ctx}>
      <div
        ref={root}
        className="flex min-h-0 flex-col"
        style={height === undefined ? undefined : { height }}
        onPointerEnter={() => setInteracting(true)}
        onPointerLeave={() => setInteracting(false)}
        onFocusCapture={() => setFocusWithin(true)}
        onBlurCapture={(e) => {
          if (!e.currentTarget.contains(e.relatedTarget)) setFocusWithin(false);
        }}
      >
        <span className="sr-only" aria-live="polite">
          {selected.length > 0
            ? `${selected.length} ${selected.length === 1 ? "process" : "processes"} selected`
            : ""}
        </span>
        {only ? (
          <div data-process-scroll className="max-h-96 min-h-0 overflow-auto">
            <table
              aria-label="Selected processes"
              className="w-full text-[12px]"
            >
              <thead>
                <tr>
                  <ProcessSelectionHead />
                  <th className="px-3 text-left font-normal">Process</th>
                  <th className="px-3 text-right font-normal">PID</th>
                  <th className="px-3 text-left font-normal">Last observed</th>
                </tr>
              </thead>
              <tbody>
                {selected.map((p) => (
                  <ProcessSelectionRow
                    key={targetKey(p)}
                    rowKey={targetKey(p)}
                    label={`${p.name} (${p.pid})`}
                    targets={[p]}
                  >
                    <td className="px-3">{p.name}</td>
                    <td className="figures px-3 text-right">{p.pid}</td>
                    <td className="px-3 text-muted-foreground">
                      {quitExclusion(p) ??
                        "Running when observed; checked again before sending"}
                    </td>
                  </ProcessSelectionRow>
                ))}
              </tbody>
            </table>
          </div>
        ) : (
          children
        )}
        {selected.length > 0 && (
          <div
            className="flex shrink-0 items-center gap-2 border-border-subtle border-t px-3 text-[11px]"
            style={{ height: SELECTION_BAR_PX }}
          >
            <span className="figures mr-auto whitespace-nowrap">
              {selected.length} selected
              {hidden > 0 && !only ? ` · ${hidden} hidden` : ""}
            </span>
            <Button
              variant="ghost"
              size="sm"
              aria-pressed={only}
              onClick={() => setOnly(!only)}
            >
              Selected only
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setState(EMPTY);
                setOnly(false);
              }}
            >
              Clear
            </Button>
            <Button
              variant="secondary"
              size="sm"
              disabled={eligible === 0 || flow.busy}
              onClick={() => flow.requestMany(selected, "quit")}
            >
              Quit {selected.length}…
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={eligible === 0 || flow.busy}
              onClick={() => flow.requestMany(selected, "force_quit")}
            >
              Force Quit {selected.length}…
            </Button>
          </div>
        )}
        {flow.results.length > 1 && (
          <details className="shrink-0 border-border-subtle border-t px-3 py-2 text-[12px]">
            <summary className="cursor-pointer">
              Batch results:{" "}
              {flow.results.filter((r) => r.state === "accepted").length} of{" "}
              {flow.results.length} requests accepted
            </summary>
            <ul className="max-h-52 overflow-auto">
              {flow.results.map((r) => (
                <li key={targetKey(r.target)} className="py-1">
                  {r.target.name}{" "}
                  <span className="figures">({r.target.pid})</span>: {r.message}
                </li>
              ))}
            </ul>
          </details>
        )}
        <QuitDialog flow={flow} />
      </div>
    </Selection.Provider>
  );
}

export function ProcessSelectionHead() {
  const ctx = useProcessSelection();
  if (!ctx) return null;
  const targets = uniqueTargets(
    ctx.allTargets.filter((p) => p.running !== false)
  );
  const count = targets.filter((p) => ctx.keys.has(targetKey(p))).length;
  return (
    <th scope="col" className="w-8 px-2">
      <Checkbox
        aria-label="Select all matching processes"
        disabled={targets.length === 0}
        className={NEUTRAL_CHECKBOX}
        checked={
          count === 0
            ? false
            : count === targets.length
              ? true
              : "indeterminate"
        }
        onClick={() => ctx.selectAll(count > 0 && count === targets.length)}
      />
    </th>
  );
}

/** Shared row chrome for both the virtualized live table and grouped usage tables. */
export function ProcessSelectionRow({
  rowKey,
  label,
  targets,
  children,
  className,
  ...props
}: ComponentProps<"tr"> & {
  rowKey: string;
  label: string;
  targets: readonly ProcessTarget[];
}) {
  const ctx = useProcessSelection();
  if (!ctx)
    return (
      <tr className={className} {...props}>
        {children}
      </tr>
    );
  const available = targets.filter((p) => p.running !== false);
  const count = available.filter((p) => ctx.keys.has(targetKey(p))).length;
  const all = available.length > 0 && count === available.length;
  const some = count > 0;
  const chosen = some ? ctx.selected : available;
  const actions = chosen.filter((p) => quitExclusion(p) === null).length;
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <tr
          {...props}
          data-process-key={rowKey}
          data-state={some ? "selected" : undefined}
          tabIndex={ctx.focused === rowKey ? 0 : -1}
          aria-selected={some}
          className={cn(
            "group h-[29px] border-border-subtle border-b hover:bg-selected focus-visible:outline-2 focus-visible:outline-ring focus-visible:-outline-offset-2 data-[state=selected]:bg-selected",
            className
          )}
          onFocus={(e) => {
            if (e.target === e.currentTarget) ctx.focus(rowKey);
          }}
          onClick={(e) => {
            if (
              e.ctrlKey ||
              (e.target instanceof HTMLElement &&
                e.target.closest(
                  "button,input,[role=checkbox],a,[data-selection-gutter]"
                ))
            )
              return;
            ctx.select(rowKey, {
              range: e.shiftKey,
              additive: e.metaKey,
              toggle: e.metaKey,
            });
          }}
          onKeyDown={(e) => {
            props.onKeyDown?.(e);
            if (!e.defaultPrevented) ctx.keyDown(e, rowKey);
          }}
          onContextMenu={() => {
            if (!some && available.length) ctx.select(rowKey);
          }}
          onPointerEnter={() => ctx.dragOver(rowKey)}
        >
          <td
            className="w-8 select-none px-2"
            data-selection-gutter
            aria-label="Selection"
            onPointerDown={(e) => {
              if (
                available.length === 0 ||
                e.button !== 0 ||
                e.ctrlKey ||
                (e.target instanceof HTMLElement && e.target.closest("button"))
              )
                return;
              e.preventDefault();
              ctx.dragStart(rowKey, e.metaKey);
            }}
          >
            <Checkbox
              disabled={!available.length}
              aria-label={`Select ${label}`}
              className={NEUTRAL_CHECKBOX}
              checked={all ? true : some ? "indeterminate" : false}
              onClick={(e) => {
                e.stopPropagation();
                ctx.select(rowKey, {
                  range: e.shiftKey,
                  additive: true,
                  toggle: true,
                });
              }}
            />
          </td>
          {children}
        </tr>
      </ContextMenuTrigger>
      <ContextMenuContent className="w-60">
        <ContextMenuLabel>
          {chosen.length > 1 ? `${chosen.length} selected processes` : label}
        </ContextMenuLabel>
        <ContextMenuSeparator />
        <ContextMenuItem
          disabled={actions === 0}
          onSelect={() => ctx.flow.requestMany(chosen, "quit")}
        >
          {chosen.length > 1 ? `Quit ${chosen.length} processes…` : "Quit…"}
        </ContextMenuItem>
        <ContextMenuItem
          variant="destructive"
          disabled={actions === 0}
          onSelect={() => ctx.flow.requestMany(chosen, "force_quit")}
        >
          {chosen.length > 1
            ? `Force Quit ${chosen.length} processes…`
            : "Force Quit…"}
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

export function SharedProcessActions({
  targets,
}: {
  targets: readonly ProcessTarget[];
}) {
  const ctx = useProcessSelection();
  if (!ctx || ctx.selected.length > 0) return null;
  const first = targets[0];
  if (targets.length === 1 && first)
    return <ProcessRowActions p={first} onRequest={ctx.flow.request} />;
  if (!targets.length) return null;
  return (
    <span className="inline-flex gap-1">
      <Button
        variant="ghost"
        size="sm"
        onClick={(e) => {
          e.stopPropagation();
          ctx.flow.requestMany(targets, "quit");
        }}
      >
        Quit {targets.length}…
      </Button>
      <Button
        variant="ghost"
        size="sm"
        onClick={(e) => {
          e.stopPropagation();
          ctx.flow.requestMany(targets, "force_quit");
        }}
      >
        Force Quit {targets.length}…
      </Button>
    </span>
  );
}
