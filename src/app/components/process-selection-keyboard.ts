import type { SelectableProcessRow } from "@core/process-selection";
import type { KeyboardEvent } from "react";
import type { SelectionContext } from "./process-selection-context";

interface SelectionKeys {
  selectAll: SelectionContext["selectAll"];
  select: SelectionContext["select"];
  focus: SelectionContext["focus"];
  clear: () => void;
  selectable: readonly SelectableProcessRow[];
  viewportHeight: number | undefined;
}
export function selectionKeyDown(
  e: KeyboardEvent,
  key: string,
  actions: SelectionKeys
) {
  const { selectAll, clear, select } = actions;
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
    clear();
    return;
  }
  if (e.key === " ") {
    e.preventDefault();
    select(key, { toggle: true, range: e.shiftKey });
    return;
  }
  navigateSelection(e, key, actions);
}
function navigateSelection(
  e: KeyboardEvent,
  key: string,
  actions: SelectionKeys
) {
  const { select, focus, selectable, viewportHeight } = actions;
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
    ? Math.max(1, Math.floor((viewportHeight ?? 290) / 29) - 2)
    : 1;
  let next = at + (e.key === "ArrowUp" || e.key === "PageUp" ? -step : step);
  if (e.key === "Home") next = 0;
  if (e.key === "End") next = selectable.length - 1;
  const row = selectable[Math.max(0, Math.min(selectable.length - 1, next))];
  if (!row) return;
  if (!e.metaKey || e.shiftKey)
    select(row.key, { range: e.shiftKey, additive: e.metaKey });
  focus(row.key);
}
