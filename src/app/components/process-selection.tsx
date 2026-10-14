import {
  holdProcessRows,
  type ProcessSelection,
  type ProcessTarget,
  type SelectableProcessRow,
  selectProcessRow,
  targetKey,
  uniqueTargets,
} from "@core/process-selection";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { QuitDialog } from "~/components/process-actions";
import { useProcessDrag } from "~/hooks/use-process-drag";
import { useQuitFlow } from "~/hooks/use-quit-flow";
import { useHostId } from "~/stores/host-store";
import { Selection, type SelectionContext } from "./process-selection-context";
import { selectionKeyDown } from "./process-selection-keyboard";
import {
  ProcessSelectionSummary,
  SelectedProcessTable,
} from "./process-selection-summary";

export { useProcessSelection } from "./process-selection-context";
export {
  ProcessSelectionHead,
  ProcessSelectionRow,
  SharedProcessActions,
} from "./process-selection-row";
export { SELECTION_BAR_PX } from "./process-selection-summary";

const EMPTY: ProcessSelection = { targets: [], anchor: null };
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
  const clear = () => {
    setState(EMPTY);
    setOnly(false);
  };
  const keyDown: SelectionContext["keyDown"] = (event, key) =>
    selectionKeyDown(event, key, {
      selectAll,
      clear,
      select,
      focus,
      selectable,
      viewportHeight: root.current?.clientHeight,
    });
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
    recordFocus: setFocused,
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
        {only ? <SelectedProcessTable /> : children}
        <ProcessSelectionSummary
          only={only}
          hidden={hidden}
          setOnly={setOnly}
          clear={clear}
        />
        <QuitDialog flow={flow} />
      </div>
    </Selection.Provider>
  );
}
